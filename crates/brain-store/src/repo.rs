//! BrainRepo: a regular git repository with an events/ subtree, one
//! JSON blob per event, one commit per append.
//!
//! v0.1 scope: single-event synchronous writes. No batching, no lease,
//! no tag snapshots, no signing. All of those land in follow-ups.

use std::path::{Path, PathBuf};

use brain_types::{
    ActorKind, Event, EventDraft, EventPayload, EventRef, EventState, EventType, IndexStatus,
    RejectReason, SCHEMA_VERSION, SignatureState,
};
use chrono::Utc;
use git2::{FileMode, ObjectType, Oid, Repository, RepositoryOpenFlags, Signature};
use tracing::debug;
use uuid::Uuid;

use crate::error::StoreError;

/// A brain stored as a regular git repository.
pub struct BrainRepo {
    path: PathBuf,
    repo: Repository,
}

impl std::fmt::Debug for BrainRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrainRepo")
            .field("path", &self.path)
            .finish()
    }
}

impl BrainRepo {
    /// Initialize a new brain at `path`. Creates the directory, `git init`s
    /// it, and writes a bootstrap commit with the SCHEMA marker.
    ///
    /// Refuses if `path` already is *any* git repository — normal, bare, or
    /// otherwise. Without this check, `brain init /path/to/bare.git` would
    /// nest a second repo inside the user's bare repo (".git" sentinel
    /// misses bare layouts). NO_SEARCH prevents walking up parent dirs.
    pub fn init<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();

        if open_repo_strict(&path).is_ok() {
            return Err(StoreError::AlreadyExists {
                path: path.display().to_string(),
            });
        }

        std::fs::create_dir_all(&path)?;
        // Tighten brain dir perms on Unix so other users on a shared host
        // can't read events/*.json or the SQLite index directly. Brains
        // are single-user by design; the prefilter is defense-in-depth,
        // not a substitute for OS-level access control. Codex F2 round 5.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let repo = Repository::init(&path)?;

        // Build the bootstrap tree in a scope so every TreeBuilder (which
        // borrows `repo`) is dropped before we move `repo` into `Self`.
        let root_tree_oid: Oid = {
            let schema_blob =
                repo.blob(format!("schema_version={}\n", SCHEMA_VERSION).as_bytes())?;
            let empty_events_tree = repo.treebuilder(None)?.write()?;

            let mut root_tb = repo.treebuilder(None)?;
            root_tb.insert("SCHEMA", schema_blob, FileMode::Blob.into())?;
            root_tb.insert("events", empty_events_tree, FileMode::Tree.into())?;
            root_tb.write()?
        };

        {
            let root_tree = repo.find_tree(root_tree_oid)?;
            let sig = bootstrap_signature()?;
            repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!(
                    "brain init\n\nSchema version {}. Version control for AI agent memory.",
                    SCHEMA_VERSION
                ),
                &root_tree,
                &[],
            )?;
        }

        debug!(path = %path.display(), "brain initialized");
        Ok(Self { path, repo })
    }

    /// Open an existing brain at `path`. Validates that the repo is actually
    /// a brain (has a SCHEMA *blob* at HEAD whose contents look like a brain
    /// marker). Refuses to attach to:
    ///   - random git repos (no SCHEMA, or SCHEMA isn't a blob, or contents
    ///     don't match — a dir named "SCHEMA" should NOT pass)
    ///   - bare repos (they have no workdir so they can't host events/)
    ///   - empty/unborn repos (no HEAD commit yet)
    ///
    /// Why strict: otherwise `--brain-dir ~/my-project` could graft an
    /// events/ tree onto the user's real repo.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let repo = open_repo_strict(&path)?;

        // Reject bare repos up front. A bare layout has no workdir, so a
        // brain's on-disk `events/` tree and the user's working copy story
        // wouldn't make sense anyway.
        if repo.is_bare() {
            return Err(StoreError::NotABrain {
                path: path.display().to_string(),
            });
        }

        // Reject detached HEAD. Writes to a detached HEAD land on an
        // anonymous commit chain that evaporates the moment the user
        // checks out any branch — notes silently disappear from `log`,
        // `search`, and the index catch-up. libgit2 accepts the commit
        // without warning, so we have to gate here (Codex R13 P2).
        if repo.head_detached().unwrap_or(false) {
            return Err(StoreError::DetachedHead {
                path: path.display().to_string(),
            });
        }

        // Scope the Tree borrow so it drops before we move `repo` into Self.
        {
            let head_tree = repo
                .head()
                .ok()
                .and_then(|h| h.peel_to_tree().ok())
                .ok_or_else(|| StoreError::NotABrain {
                    path: path.display().to_string(),
                })?;
            validate_brain_tree(&repo, &head_tree, &path)?;
        }

        Ok(Self { path, repo })
    }

    /// The repo's on-disk path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of commits reachable from HEAD. Useful for tests and doctor.
    pub fn commit_count(&self) -> Result<usize, StoreError> {
        let walk = self.head_revwalk()?;
        Ok(walk.count())
    }

    /// Revwalk pinned to topological order so replay semantics are
    /// deterministic even under rewritten or non-linear history
    /// (Codex R13 P1). libgit2's default revwalk is `Sort::NONE`, which
    /// may return commits in any order; that's safe for linear history
    /// (which is all we produce via `append_event`) but fragile under
    /// manual rewrites, imports, or future merge commits. TOPOLOGICAL
    /// guarantees parents appear after their children in the returned
    /// stream, so `.reverse()` on the collected result yields oldest-
    /// first deterministically.
    fn head_revwalk(&self) -> Result<git2::Revwalk<'_>, StoreError> {
        let mut walk = self.repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        walk.push_head()?;
        Ok(walk)
    }

    /// Append a single event. Synchronous, one commit per call. Returns an
    /// `EventRef` with `state = Committed`.
    ///
    /// Pipeline:
    ///   1. shallow_validate the draft (schema version, idempotency key, type)
    ///   2. CHECK IDEMPOTENCY: if a prior event carries the same
    ///      idempotency_key, return its EventRef with was_idempotent_replay=true.
    ///      No new commit is written.
    ///   3. allocate event_id (UUIDv7)
    ///   4. build the Event struct in memory (commit_oid empty, to be filled
    ///      by readers from the commit that lands it)
    ///   5. serialize to JSON
    ///   6. write the blob + update events/ subtree + commit
    ///   7. return EventRef with the commit oid
    pub fn append_event(&self, draft: EventDraft) -> Result<EventRef, StoreError> {
        draft.shallow_validate(SCHEMA_VERSION)?;

        // Serialize append attempts per brain repo. The retry loop below
        // handles libgit2 HEAD CAS failures, but it cannot close this race:
        // writer A and writer B both check idempotency before either commits;
        // A commits; B reads A's commit as parent and also commits. A narrow
        // lock around the whole append keeps idempotency lookup + commit
        // atomic across processes without changing the event model.
        let _append_lock = self.append_lock()?;

        // Multi-writer race. Two shapes to absorb:
        //   - `Locked` (-14): libgit2 couldn't take `.git/refs/heads/<br>.lock`
        //     because another writer holds it.
        //   - `Modified` (-15): libgit2 uses compare-and-swap on HEAD at
        //     commit time. If another writer committed between our `head()`
        //     read and our `commit(&[&parent_commit])`, our parent oid no
        //     longer matches and git rejects the commit. In practice this
        //     is the common one on SSDs.
        //
        // Both are safe to retry: the idempotency key stops us from
        // double-landing, and the next attempt reads the new HEAD as its
        // parent so our commit stacks on top of whoever won this round.
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut delay = Duration::from_millis(25);
        loop {
            match self.append_event_once(&draft) {
                Ok(er) => return Ok(er),
                Err(StoreError::Git(ref ge))
                    if matches!(
                        ge.code(),
                        git2::ErrorCode::Locked | git2::ErrorCode::Modified
                    ) && Instant::now() < deadline =>
                {
                    std::thread::sleep(delay);
                    delay = std::cmp::min(delay * 2, Duration::from_millis(200));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// One attempt at an append. Callers wrap this in a retry loop for
    /// multi-writer lock contention.
    fn append_event_once(&self, draft: &EventDraft) -> Result<EventRef, StoreError> {
        // Idempotency check: return the prior EventRef if this key was already used.
        if let Some(prior) = self.find_by_idempotency_key(&draft.idempotency_key)? {
            return Ok(prior);
        }

        let event_id = Uuid::now_v7();
        let time_recorded = Utc::now();
        let time_observed = draft.time_observed.unwrap_or(time_recorded);

        let event = Event {
            event_id,
            event_type: draft.event_type.clone(),
            subject: draft.subject.clone(),
            parent_event_id: draft.parent_event_id,
            chain_id: draft.chain_id,
            actor: draft.actor.clone(),
            commit_oid: String::new(), // filled at read time from enclosing commit
            time_observed,
            time_recorded,
            layer: draft.layer.clone(),
            authority: draft.authority.clone(),
            signature_state: SignatureState::Unsigned,
            classification: draft.classification.clone(),
            payload: draft.payload.clone(),
            schema_version: SCHEMA_VERSION,
            idempotency_key: Some(draft.idempotency_key.clone()),
        };

        // Serialize FIRST, then scan the serialized JSON. This covers every
        // string field on every payload variant (present and future),
        // including nested serde_json::Value content that an allow-list of
        // handpicked fields would miss. Reject before the blob is written —
        // nothing touches git yet.
        let json = serde_json::to_vec_pretty(&event)?;

        // Size cap: reject oversized events at write time so we don't
        // later poison every reader. `deserialize_event_blob` enforces
        // the same cap on reads; aligning the two prevents an event
        // that committed successfully from becoming permanently
        // unreadable. Codex round 4 P2: "write path had no cap."
        if json.len() > MAX_EVENT_BLOB_BYTES {
            return Err(StoreError::OversizedBlob {
                bytes: json.len(),
                cap: MAX_EVENT_BLOB_BYTES,
            });
        }

        let json_str = std::str::from_utf8(&json)
            .map_err(|e| StoreError::Unexpected(format!("event JSON not utf8: {e}")))?;
        if let Some(pattern) = crate::secrets::detect_secret_text(json_str) {
            return Err(StoreError::Rejected(RejectReason::SecretDetected {
                pattern: pattern.to_string(),
            }));
        }

        // Refuse writes on detached HEAD even if the runtime was opened
        // while attached — someone may have run `git checkout <sha>` on
        // the brain dir out from under us. Commits on detached HEAD
        // evaporate on next branch switch (Codex R13 P2).
        if self.repo.head_detached().unwrap_or(false) {
            return Err(StoreError::DetachedHead {
                path: self.path.display().to_string(),
            });
        }

        let blob_oid = self.repo.blob(&json)?;

        // Extend the events subtree with this new file.
        let parent_commit = self.repo.head()?.peel_to_commit()?;
        let parent_tree = parent_commit.tree()?;

        // Revalidate HEAD is still a brain before grafting events/ onto it.
        // Closes a TOCTOU window where open() validated, then a concurrent
        // `git reset` repointed HEAD at a non-brain commit. Cheap check,
        // catastrophic if skipped (write lands in someone's real repo).
        validate_brain_tree(&self.repo, &parent_tree, &self.path)?;

        let parent_events_tree = match parent_tree.get_name("events") {
            Some(entry) if entry.kind() == Some(git2::ObjectType::Tree) => {
                Some(self.repo.find_tree(entry.id())?)
            }
            _ => None,
        };

        let mut events_tb = self.repo.treebuilder(parent_events_tree.as_ref())?;
        let filename = format!("{}.json", event_id);
        events_tb.insert(&filename, blob_oid, FileMode::Blob.into())?;
        let events_tree_oid = events_tb.write()?;

        let mut root_tb = self.repo.treebuilder(Some(&parent_tree))?;
        root_tb.insert("events", events_tree_oid, FileMode::Tree.into())?;
        let root_tree_oid = root_tb.write()?;
        let root_tree = self.repo.find_tree(root_tree_oid)?;

        let sig = signature_from_actor(&draft.actor)?;
        let commit_msg = format_commit_message(&event);

        let commit_oid: Oid = self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &commit_msg,
            &root_tree,
            &[&parent_commit],
        )?;

        Ok(EventRef {
            event_id,
            state: EventState::Committed,
            commit_oid: Some(commit_oid.to_string()),
            // No index exists yet in v0.1; Ok is the truthful answer because
            // "no index" means no lag to report. Index lag/degradation will
            // show up here once brain-index is wired in.
            index_status: IndexStatus::Ok,
            reached_at: Utc::now(),
            was_idempotent_replay: false,
        })
    }

    fn append_lock(&self) -> Result<AppendLock, StoreError> {
        use std::time::{Duration, Instant};

        let lock_path = self.repo.path().join("brain-append.lock.d");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut delay = Duration::from_millis(10);
        loop {
            match std::fs::create_dir(&lock_path) {
                Ok(()) => return Ok(AppendLock { path: lock_path }),
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(delay);
                    delay = std::cmp::min(delay * 2, Duration::from_millis(100));
                }
                Err(e) => return Err(StoreError::Io(e)),
            }
        }
    }

    /// Hex oid of the current HEAD commit. Cheap — doesn't walk history.
    /// Used by the index catch-up fast-path to detect "nothing to do."
    pub fn head_commit_oid(&self) -> Result<String, StoreError> {
        Ok(self.repo.head()?.peel_to_commit()?.id().to_string())
    }

    /// Count of commits that actually introduced an event — trailer
    /// present AND blob newly added in that commit (not inherited
    /// unchanged from a parent). Routes through `event_from_commit` so
    /// the inherited-blob skip and blob/trailer cross-check apply here
    /// too; a forged commit that reuses an old trailer + old blob can't
    /// inflate this count (Codex R11 P1 #3).
    ///
    /// Cost: walks HEAD and does one blob read per event-carrying commit.
    /// At personal-brain scale (< 100k events) this is fast; if it
    /// matters later, a trailer-cached projection in the index is the
    /// right answer.
    pub fn event_count(&self) -> Result<usize, StoreError> {
        let walk = self.head_revwalk()?;
        let mut n = 0usize;
        for oid_result in walk {
            let oid = oid_result?;
            let commit = self.repo.find_commit(oid)?;
            if self.event_from_commit(&commit)?.is_some() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Walk every event in history, oldest-first. Used by the index
    /// `rebuild_from` path — replaying in commit order keeps projection
    /// updates monotonic (latest wins).
    pub fn events_oldest_first(&self) -> Result<Vec<Event>, StoreError> {
        let walk = self.head_revwalk()?;
        let mut result = Vec::new();
        for oid_result in walk {
            let oid = oid_result?;
            let commit = self.repo.find_commit(oid)?;
            if let Some(event) = self.event_from_commit(&commit)? {
                result.push(event);
            }
        }
        result.reverse();
        Ok(result)
    }

    /// Events committed strictly after `last_oid`, oldest-first.
    /// If `last_oid` is None or can't be resolved, returns every event.
    /// Used by the index catch-up path — compare the watermark to HEAD
    /// and feed the delta to `BrainIndex::ingest`.
    pub fn events_after(&self, last_oid: Option<&str>) -> Result<Vec<Event>, StoreError> {
        let all = self.events_oldest_first()?;
        let Some(oid_str) = last_oid else {
            return Ok(all);
        };
        // Find the position of last_oid in the chain and return everything strictly after.
        // Linear scan; fine at personal-brain scale (< 100k events).
        let cut = all.iter().position(|e| e.commit_oid == oid_str);
        Ok(match cut {
            Some(i) => all.into_iter().skip(i + 1).collect(),
            // last_oid wasn't found in history (history was rewritten, or
            // the watermark is from a different brain). Safer to return
            // everything and let the caller full-rebuild.
            None => all,
        })
    }

    /// List events from the history, newest-first, up to `limit`. Skips
    /// commits that don't carry an `event_id:` trailer (e.g. bootstrap,
    /// or future admin commits).
    pub fn list_events(&self, limit: usize) -> Result<Vec<Event>, StoreError> {
        let walk = self.head_revwalk()?;
        let mut result = Vec::new();
        for oid_result in walk {
            if result.len() >= limit {
                break;
            }
            let oid = oid_result?;
            let commit = self.repo.find_commit(oid)?;
            if let Some(event) = self.event_from_commit(&commit)? {
                result.push(event);
            }
        }
        Ok(result)
    }

    /// Dumb substring search over event payload text. Matches newest-first
    /// up to `limit`. Case-insensitive. Looks inside summary (OBSERVE),
    /// claim content (CLAIM), lesson text (LESSON), preference key+value
    /// (PREF), skill filename (SKILL_EDIT), etc.
    ///
    /// This is the v0.1 stand-in for real retrieval. brain-index with
    /// BM25 + sqlite-vec replaces this once it lands.
    pub fn search_events(&self, needle: &str, limit: usize) -> Result<Vec<Event>, StoreError> {
        let needle_lower = needle.to_lowercase();
        let walk = self.head_revwalk()?;
        let mut result = Vec::new();
        for oid_result in walk {
            if result.len() >= limit {
                break;
            }
            let oid = oid_result?;
            let commit = self.repo.find_commit(oid)?;
            let Some(event) = self.event_from_commit(&commit)? else {
                continue;
            };
            if event_matches_needle(&event, &needle_lower) {
                result.push(event);
            }
        }
        Ok(result)
    }

    /// Find a prior event by its `idempotency_key`. Walks HEAD backwards.
    /// Returns the EventRef with `was_idempotent_replay: true` if found.
    ///
    /// v0.1 is O(n) over history. At 10k+ events per brain this will want
    /// a derived index. Acceptable for personal-brain scale.
    pub fn find_by_idempotency_key(&self, key: &str) -> Result<Option<EventRef>, StoreError> {
        let walk = self.head_revwalk()?;
        for oid_result in walk {
            let oid = oid_result?;
            let commit = self.repo.find_commit(oid)?;
            let Some(event) = self.event_from_commit(&commit)? else {
                continue;
            };
            if event.idempotency_key.as_deref() == Some(key) {
                return Ok(Some(EventRef {
                    event_id: event.event_id,
                    state: EventState::Committed,
                    commit_oid: Some(event.commit_oid),
                    index_status: IndexStatus::Ok,
                    reached_at: Utc::now(),
                    was_idempotent_replay: true,
                }));
            }
        }
        Ok(None)
    }

    /// Load the event added by a single commit using the `event_id:` trailer
    /// in the commit message. Returns Ok(None) when the commit doesn't carry
    /// that trailer (bootstrap, admin commits) OR when events/<id>.json
    /// already existed in any parent tree (i.e. this commit didn't
    /// actually introduce the event).
    ///
    /// Two forgery defenses (Codex R11 P1 + R12 P1):
    ///   1. Blob/trailer cross-check: the blob's internal `event_id` MUST
    ///      match the trailer event_id. Otherwise a tampered commit could
    ///      point its trailer at one blob but claim another.
    ///   2. Filename-in-parent skip: every legitimate append writes a
    ///      UUIDv7-named file that has never existed before, so if
    ///      events/<id>.json is present in ANY parent tree, this commit
    ///      didn't introduce the event — it either inherited the blob
    ///      unchanged (trailer-reuse attack) or rewrote it in place
    ///      (blob-content-swap attack, where the rewritten blob still
    ///      claims the same internal event_id and so passes check #1).
    ///      Either way, return None.
    ///
    /// Both checks are cheap (one tree lookup per parent) and only fire
    /// under deliberate tampering, so normal operation pays nothing.
    fn event_from_commit(&self, commit: &git2::Commit<'_>) -> Result<Option<Event>, StoreError> {
        let Some(trailer_event_id) = event_id_from_commit_message(commit.message().unwrap_or(""))
        else {
            return Ok(None);
        };
        let tree = commit.tree()?;
        let events_entry = match tree.get_name("events") {
            Some(e) if e.kind() == Some(git2::ObjectType::Tree) => e,
            _ => return Ok(None),
        };
        let events_tree = self.repo.find_tree(events_entry.id())?;
        let filename = format!("{}.json", trailer_event_id);
        let file_entry = match events_tree.get_name(&filename) {
            Some(e) => e,
            None => return Ok(None),
        };
        let blob_oid = file_entry.id();

        // Defense #2: if events/<id>.json exists in any parent tree,
        // this commit did NOT introduce the event. Legitimate appends
        // always write a fresh UUIDv7-named file that has never existed
        // before, so the filename in parent is the tell. Catches both
        // shapes of forgery: same-blob reuse (inherited unchanged) AND
        // blob-content-swap (rewritten in place with a blob whose
        // internal event_id still matches the trailer, which would
        // otherwise pass the cross-check). Walk every parent — normally
        // one; libgit2 supports merges though we don't create them.
        for parent in commit.parents() {
            let Ok(parent_tree) = parent.tree() else {
                continue;
            };
            let Some(parent_events_entry) = parent_tree.get_name("events") else {
                continue;
            };
            if parent_events_entry.kind() != Some(git2::ObjectType::Tree) {
                continue;
            }
            let Ok(parent_events_tree) = self.repo.find_tree(parent_events_entry.id()) else {
                continue;
            };
            if parent_events_tree.get_name(&filename).is_some() {
                // Filename existed upstream. Not a new event commit.
                return Ok(None);
            }
        }

        let blob = self.repo.find_blob(blob_oid)?;
        let mut event: Event = deserialize_event_blob(&blob)?;

        // Defense #1: the blob's internal event_id must match the trailer.
        if event.event_id != trailer_event_id {
            return Err(StoreError::ForgedCommit {
                oid: commit.id().to_string(),
                detail: format!(
                    "trailer claims event_id {trailer_event_id} but blob {filename} contains event_id {}",
                    event.event_id,
                ),
            });
        }

        event.commit_oid = commit.id().to_string();
        Ok(Some(event))
    }

    /// Read the event introduced by a given commit. Enriches `commit_oid`
    /// from the commit parameter since it isn't serialized on disk. Returns
    /// None when the commit did not introduce `event_id`.
    ///
    /// This intentionally routes through `event_from_commit` so reads use
    /// the same trailer parsing, filename-in-parent skip, and blob/trailer
    /// cross-check as list/count/idempotency. The `events/` tree is
    /// cumulative, but inherited files are not "the event introduced by
    /// this commit" and are hidden here.
    pub fn read_event(
        &self,
        commit_oid: &str,
        event_id: Uuid,
    ) -> Result<Option<Event>, StoreError> {
        let oid = Oid::from_str(commit_oid).map_err(|_| StoreError::BadOid {
            oid: commit_oid.to_string(),
        })?;
        let commit = self.repo.find_commit(oid)?;
        Ok(self
            .event_from_commit(&commit)?
            .filter(|event| event.event_id == event_id))
    }
}

struct AppendLock {
    path: PathBuf,
}

impl Drop for AppendLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}

/// Max bytes we'll accept for a single event blob before giving up.
/// Events are metadata + a short payload — 1 MiB is absurdly generous.
/// Without a cap, a corrupted or malicious 4 GB blob would stall an
/// `open` / `log` / `catch_up_index` walk while serde_json parses it
/// into memory.
const MAX_EVENT_BLOB_BYTES: usize = 1024 * 1024;

fn deserialize_event_blob(blob: &git2::Blob<'_>) -> Result<Event, StoreError> {
    let bytes = blob.content();
    if bytes.len() > MAX_EVENT_BLOB_BYTES {
        return Err(StoreError::OversizedBlob {
            bytes: bytes.len(),
            cap: MAX_EVENT_BLOB_BYTES,
        });
    }
    Ok(serde_json::from_slice(bytes)?)
}

/// Does any searchable text on this event contain `needle_lower`?
/// (needle should already be lowercase; we lowercase each haystack field
/// once per call, avoiding allocating an owned lowercase version of every
/// large field.)
fn event_matches_needle(event: &Event, needle_lower: &str) -> bool {
    use brain_types::EventPayload;
    let haystacks: Vec<String> = match &event.payload {
        EventPayload::Observe(o) => {
            let mut v = vec![o.summary.clone()];
            if let Some(c) = &o.content {
                v.push(c.clone());
            }
            if let Some(s) = &o.source {
                v.push(s.clone());
            }
            v
        }
        EventPayload::Claim(c) => {
            let mut v = vec![c.schema_type.clone(), c.key.clone()];
            if let Ok(content_str) = serde_json::to_string(&c.content) {
                v.push(content_str);
            }
            v
        }
        EventPayload::Lesson(l) => vec![l.category.clone(), l.text.clone()],
        EventPayload::Pref(p) => {
            let mut v = vec![p.category.clone(), p.key.clone()];
            if let Ok(value_str) = serde_json::to_string(&p.value) {
                v.push(value_str);
            }
            v
        }
        EventPayload::SkillEdit(s) => vec![
            s.skill_name.clone(),
            s.file.clone(),
            s.reason.clone().unwrap_or_default(),
        ],
        EventPayload::Verify(v) => vec![v.attestation.clone()],
        EventPayload::Archive(a) => vec![a.reason.clone().unwrap_or_default()],
        EventPayload::Redact(r) => vec![r.reason.clone()],
        EventPayload::Import(i) => vec![i.manifest_hash.clone()],
        EventPayload::Audit(a) => vec![a.detail.clone(), format!("{:?}", a.kind)],
    };
    haystacks
        .iter()
        .any(|h| h.to_lowercase().contains(needle_lower))
}

/// Parse the `event_id:` trailer from a commit message formatted by this
/// crate. Only considers the TRAILER BLOCK (lines after the last blank
/// line), so a subject or body line that happens to contain `event_id:`
/// cannot redirect readers to a different blob (Codex C1: trailer
/// injection defense-in-depth; combined with newline-stripping at
/// `payload_summary`, an attacker has no way to smuggle a forged trailer).
fn event_id_from_commit_message(msg: &str) -> Option<Uuid> {
    let trailer_block = trailer_block_of(msg);
    for line in trailer_block.lines() {
        if let Some(rest) = line.strip_prefix("event_id: ") {
            return Uuid::parse_str(rest.trim()).ok();
        }
    }
    None
}

/// Return the trailer block of a commit message, which is the final
/// paragraph of the message (everything after the last blank line). If
/// there are no blank lines, returns an empty string — a message without
/// a separate trailer block has no trailers, by convention.
fn trailer_block_of(msg: &str) -> &str {
    // Find the last blank line. `rfind("\n\n")` gets us the start of the
    // final paragraph. If the message has no blank line at all, it has
    // no trailer block.
    match msg.rfind("\n\n") {
        Some(idx) => &msg[idx + 2..],
        None => "",
    }
}

/// Open the git repository at `path` without walking upwards. Maps git2's
/// NotFound to our `StoreError::NotFound` so callers can distinguish
/// "no repo here" from "repo exists but isn't usable as a brain".
///
/// NO_SEARCH is load-bearing: if the user passes `--brain-dir ~/my-project/docs`
/// and `~/my-project/.git` exists, vanilla `Repository::open` would climb up
/// and attach to the outer repo. We don't want that. The requested path is
/// the only path we consider.
fn open_repo_strict(path: &Path) -> Result<Repository, StoreError> {
    match Repository::open_ext(
        path,
        RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&std::ffi::OsStr>(),
    ) {
        Ok(repo) => Ok(repo),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Err(StoreError::NotFound {
            path: path.display().to_string(),
        }),
        Err(e) => Err(StoreError::Git(e)),
    }
}

/// Is the given tree a brain root? Verifies:
///   - `SCHEMA` exists and is a **blob** (not a subtree named SCHEMA)
///   - the blob parses as `schema_version=<integer>` AND the integer
///     equals the compile-time `SCHEMA_VERSION`. A future-version brain
///     is refused with `InvalidState` so an older binary can't open and
///     silently append events at an older schema level. A garbage blob
///     ("schema_version=" / "schema_version=garbage") is refused as
///     `NotABrain`. Codex R2 round 5: "parse and enforce the SCHEMA
///     version when opening a repo".
///   - `events` exists and is a **tree** (not a file)
fn validate_brain_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    path: &Path,
) -> Result<(), StoreError> {
    let not_a_brain = || StoreError::NotABrain {
        path: path.display().to_string(),
    };

    let schema_entry = tree.get_name("SCHEMA").ok_or_else(not_a_brain)?;
    if schema_entry.kind() != Some(ObjectType::Blob) {
        return Err(not_a_brain());
    }
    let blob = repo.find_blob(schema_entry.id())?;
    let raw = std::str::from_utf8(blob.content()).map_err(|_| not_a_brain())?;
    // Expect exactly one line: `schema_version=<u32>[\n]`
    let first_line = raw.lines().next().ok_or_else(not_a_brain)?;
    let num = first_line
        .strip_prefix("schema_version=")
        .ok_or_else(not_a_brain)?;
    let on_disk: u32 = num.parse().map_err(|_| not_a_brain())?;
    if on_disk != SCHEMA_VERSION {
        return Err(StoreError::SchemaMismatch {
            path: path.display().to_string(),
            on_disk,
            expected: SCHEMA_VERSION,
        });
    }

    match tree.get_name("events") {
        Some(entry) if entry.kind() == Some(ObjectType::Tree) => Ok(()),
        _ => Err(not_a_brain()),
    }
}

fn bootstrap_signature<'a>() -> Result<Signature<'a>, StoreError> {
    Ok(Signature::now("brain", "brain@localhost")?)
}

fn signature_from_actor<'a>(actor: &brain_types::Actor) -> Result<Signature<'a>, StoreError> {
    let display_name = match actor.kind {
        ActorKind::Agent => format!("agent:{}", actor.id),
        ActorKind::Human => actor.id.clone(),
        ActorKind::System => format!("system:{}", actor.id),
    };
    // Sanitize to something git won't reject as an email.
    let sanitized_id: String = actor
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let email = format!("{}@brain.local", sanitized_id);
    Ok(Signature::now(&display_name, &email)?)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod forgery_tests {
    //! Codex R11 P1 #3: defend against commits that reuse event_id
    //! trailers with inherited blobs or mismatched blob contents. These
    //! tests forge commits directly via git2 so we can exercise the
    //! defenses in `event_from_commit` / `read_event` without relying
    //! on the normal append path (which already prevents this).
    use super::*;
    use brain_types::{
        Actor, ActorKind, Authority, AuthoritySource, Classification, EventDraft, EventType, Layer,
        ObservePayload, SubjectRef,
    };
    use git2::{FileMode, Repository, Signature};
    use tempfile::TempDir;

    fn sample_draft(idem: &str, summary: &str) -> EventDraft {
        EventDraft {
            event_type: EventType::Observe,
            subject: SubjectRef::Entity { id: Uuid::now_v7() },
            parent_event_id: None,
            chain_id: None,
            actor: Actor {
                kind: ActorKind::Agent,
                id: "test".into(),
                harness: None,
                signing_key_fingerprint: None,
            },
            time_observed: None,
            layer: Layer::Episodic,
            authority: Authority {
                source_kind: AuthoritySource::Agent,
                score: None,
                attested_by: None,
            },
            classification: Classification::Private,
            payload: EventPayload::Observe(ObservePayload {
                summary: summary.into(),
                content: None,
                content_ref: None,
                source: None,
            }),
            schema_version: SCHEMA_VERSION,
            idempotency_key: idem.into(),
        }
    }

    /// Build a forged commit on top of `parent_oid` with the given tree
    /// (typically inherited verbatim) and commit message. Returns the
    /// forged commit's oid.
    fn forge_commit(
        repo: &Repository,
        parent_oid: git2::Oid,
        tree_oid: git2::Oid,
        message: &str,
    ) -> git2::Oid {
        let sig = Signature::now("forger", "forger@example").unwrap();
        let parent = repo.find_commit(parent_oid).unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .unwrap()
    }

    #[test]
    fn event_count_does_not_double_count_inherited_blobs() {
        // Forge a commit that reuses E1's trailer + E1's blob (unchanged
        // from parent). Without the inherited-blob skip, E1 would be
        // counted twice — once for its real commit, once for the forged
        // one — and `find_by_idempotency_key` could return the forged
        // commit_oid for future idempotent writes.
        let td = TempDir::new().unwrap();
        let brain = BrainRepo::init(td.path().join("brain")).unwrap();
        let e1 = brain.append_event(sample_draft("e1", "one")).unwrap();
        assert_eq!(brain.event_count().unwrap(), 1);

        // Reach into the underlying repo and forge a commit that
        // inherits the current tree verbatim but carries an event_id
        // trailer claiming E1 was introduced here.
        let repo = Repository::open(brain.path()).unwrap();
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        let inherited_tree_oid = head_commit.tree().unwrap().id();
        let forged_msg = format!(
            "observe forged\n\nevent_id: {}\nschema_version: {}\n",
            e1.event_id, SCHEMA_VERSION,
        );
        forge_commit(&repo, head_commit.id(), inherited_tree_oid, &forged_msg);

        // The forged commit exists and carries an event_id trailer, but
        // its events/E1.json blob oid is identical to the parent's, so
        // it should be ignored by the defense in `event_from_commit`.
        assert_eq!(
            brain.event_count().unwrap(),
            1,
            "forged commit must not inflate event_count"
        );
        let all = brain.events_oldest_first().unwrap();
        assert_eq!(all.len(), 1, "events_oldest_first must not double-count");
    }

    #[test]
    fn event_count_rejects_blob_content_swap_forgery() {
        // Codex R12 P1: a tampered commit that REWRITES events/E1.json
        // with a different blob (content chosen so its internal event_id
        // still says E1, so the trailer cross-check passes) would pass
        // the old same-blob-oid inheritance check and inflate counts.
        // The stricter filename-in-parent check rejects it regardless of
        // blob content.
        let td = TempDir::new().unwrap();
        let brain = BrainRepo::init(td.path().join("brain")).unwrap();
        let e1 = brain.append_event(sample_draft("e1", "one")).unwrap();
        assert_eq!(brain.event_count().unwrap(), 1);

        // Fabricate a rewritten blob that carries the SAME event_id but
        // a tampered-looking summary.
        let repo = Repository::open(brain.path()).unwrap();
        let forged_event_json = serde_json::json!({
            "event_id": e1.event_id,
            "event_type": "observe",
            "subject": { "kind": "entity", "id": Uuid::now_v7() },
            "parent_event_id": null,
            "chain_id": null,
            "actor": { "kind": "agent", "id": "forger", "harness": null, "signing_key_fingerprint": null },
            "commit_oid": "",
            "time_observed": chrono::Utc::now(),
            "time_recorded": chrono::Utc::now(),
            "layer": "episodic",
            "authority": { "source_kind": "agent", "score": null, "attested_by": null },
            "signature_state": "unsigned",
            "classification": "private",
            "payload": { "Observe": { "summary": "TAMPERED", "content": null, "content_ref": null, "source": null } },
            "schema_version": SCHEMA_VERSION,
            "idempotency_key": "forged"
        });
        let forged_bytes = serde_json::to_vec(&forged_event_json).unwrap();
        let forged_blob_oid = repo.blob(&forged_bytes).unwrap();

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let parent_tree = head.tree().unwrap();
        let parent_events = parent_tree.get_name("events").unwrap();
        let parent_events_tree = repo.find_tree(parent_events.id()).unwrap();

        // Overwrite events/E1.json with the forged blob (different oid,
        // same filename, same internal event_id).
        let filename = format!("{}.json", e1.event_id);
        let mut events_tb = repo.treebuilder(Some(&parent_events_tree)).unwrap();
        events_tb
            .insert(&filename, forged_blob_oid, FileMode::Blob.into())
            .unwrap();
        let events_tree_oid = events_tb.write().unwrap();

        let mut root_tb = repo.treebuilder(Some(&parent_tree)).unwrap();
        root_tb
            .insert("events", events_tree_oid, FileMode::Tree.into())
            .unwrap();
        let root_tree_oid = root_tb.write().unwrap();

        let forged_msg = format!(
            "observe forged\n\nevent_id: {}\nschema_version: {}\n",
            e1.event_id, SCHEMA_VERSION,
        );
        forge_commit(&repo, head.id(), root_tree_oid, &forged_msg);

        // The forged commit is newer, has a valid trailer, and its blob
        // internally claims event_id E1. But events/E1.json existed in
        // the parent, so the filename-in-parent check skips it.
        assert_eq!(
            brain.event_count().unwrap(),
            1,
            "blob-content-swap must not inflate event_count"
        );
    }

    #[test]
    fn read_event_rejects_blob_id_mismatch() {
        // Forge a commit where events/E2.json points at E1's blob (so
        // the filename says E2 but the blob contents say E1). The
        // read_event cross-check must reject this as InvalidState.
        let td = TempDir::new().unwrap();
        let brain = BrainRepo::init(td.path().join("brain")).unwrap();
        let e1 = brain.append_event(sample_draft("e1", "one")).unwrap();
        let _e2 = brain.append_event(sample_draft("e2", "two")).unwrap();

        // Find E1's blob oid from its commit.
        let repo = Repository::open(brain.path()).unwrap();
        let e1_commit = repo
            .find_commit(git2::Oid::from_str(e1.commit_oid.as_deref().unwrap()).unwrap())
            .unwrap();
        let e1_blob_oid = {
            let tree = e1_commit.tree().unwrap();
            let events = tree.get_name("events").unwrap();
            let et = repo.find_tree(events.id()).unwrap();
            et.get_name(&format!("{}.json", e1.event_id)).unwrap().id()
        };

        // Forge a fake E3 event_id and build a tree where events/E3.json
        // points at E1's blob. Use a fresh UUID for the trailer.
        let fake_event_id = Uuid::now_v7();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let parent_tree = head.tree().unwrap();
        let parent_events = parent_tree.get_name("events").unwrap();
        let parent_events_tree = repo.find_tree(parent_events.id()).unwrap();

        let mut events_tb = repo.treebuilder(Some(&parent_events_tree)).unwrap();
        events_tb
            .insert(
                format!("{}.json", fake_event_id),
                e1_blob_oid,
                FileMode::Blob.into(),
            )
            .unwrap();
        let events_tree_oid = events_tb.write().unwrap();

        let mut root_tb = repo.treebuilder(Some(&parent_tree)).unwrap();
        root_tb
            .insert("events", events_tree_oid, FileMode::Tree.into())
            .unwrap();
        let root_tree_oid = root_tb.write().unwrap();

        let forged_msg = format!(
            "observe forged\n\nevent_id: {}\nschema_version: {}\n",
            fake_event_id, SCHEMA_VERSION,
        );
        let forged_oid = forge_commit(&repo, head.id(), root_tree_oid, &forged_msg);

        // read_event asks for fake_event_id at the forged commit — the
        // blob at events/<fake>.json actually contains E1's event_id.
        // Cross-check must reject.
        let err = brain
            .read_event(&forged_oid.to_string(), fake_event_id)
            .unwrap_err();
        match err {
            StoreError::ForgedCommit { detail, .. } => {
                assert!(
                    detail.contains("contains event_id"),
                    "unexpected detail: {detail}"
                );
            }
            other => panic!("expected ForgedCommit, got {other:?}"),
        }
    }
}

fn format_commit_message(event: &Event) -> String {
    let tag = match event.event_type {
        EventType::Observe => "observe",
        EventType::Claim => "claim",
        EventType::Lesson => "lesson",
        EventType::Pref => "pref",
        EventType::SkillEdit => "skill-edit",
        EventType::Verify => "verify",
        EventType::Archive => "archive",
        EventType::Redact => "redact",
        EventType::Import => "import",
        EventType::Audit => "audit",
    };
    let summary = payload_summary(&event.payload, &event.event_id);
    format!(
        "{tag}: {summary}\n\nevent_id: {}\nschema_version: {}\n",
        event.event_id, event.schema_version
    )
}

/// Subject line for the commit message. Deliberately DOES NOT include
/// free-form user text for Observe / Lesson / Redact payloads. Git
/// surfaces commit subjects everywhere (`git log --oneline`, hosting UIs,
/// tooling), so a secret that slips past the prefilter should not be
/// promoted to a second retrieval channel here. For variants where the
/// visible tokens ARE domain schema (Claim `schema_type/key`, Pref
/// `category/key`, SkillEdit `skill_name file`), we keep them because
/// they carry audit value and are not arbitrary free-form text.
///
/// Newlines and CRs are stripped belt-and-braces so no caller can
/// smuggle a forged `\nevent_id:` trailer past `event_id_from_commit_
/// message` (Codex C1 in this session: commit-trailer injection).
fn payload_summary(payload: &EventPayload, event_id: &Uuid) -> String {
    const MAX: usize = 80;
    let raw = match payload {
        // Observe.summary, Lesson.text, Redact.reason are free-form user
        // text. Use the event_id as the subject identifier instead of
        // echoing that content into the commit subject.
        EventPayload::Observe(_) => format!("observe {}", event_id),
        EventPayload::Lesson(_) => format!("lesson {}", event_id),
        EventPayload::Redact(p) => format!("redact {}", p.target_entity),
        // The remaining variants expose only domain-schema tokens or IDs
        // (no arbitrary user free-form). Keep as-is for audit value.
        EventPayload::Claim(p) => format!("{}/{}", p.schema_type, p.key),
        EventPayload::Pref(p) => format!("{}/{}", p.category, p.key),
        EventPayload::SkillEdit(p) => format!("{} {}", p.skill_name, p.file),
        EventPayload::Verify(p) => format!("verify {}", p.target_entity),
        EventPayload::Archive(p) => format!("archive {}", p.target_entity),
        EventPayload::Import(p) => format!("import {} items", p.item_count),
        EventPayload::Audit(p) => format!("audit: {:?}", p.kind),
    };
    // Strip newline-like characters so nothing can inject a trailer.
    let sanitized: String = raw
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if sanitized.chars().count() <= MAX {
        sanitized
    } else {
        let truncated: String = sanitized.chars().take(MAX - 1).collect();
        format!("{}…", truncated)
    }
}
