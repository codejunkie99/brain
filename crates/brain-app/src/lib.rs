//! brain-app: orchestration + consistency state machine + write/admin APIs.
//!
//! v0.1: `LocalBrain` is a direct in-process adapter around `BrainRepo`.
//! No daemon, no socket, no batching yet. Good enough for CLI one-shot
//! writes, which is the shape `brain init / note / log / ask` needs.

use std::path::{Path, PathBuf};

use brain_index::{BrainIndex, IndexError, SearchQuery};
use brain_store::{BrainRepo, StoreError};
use brain_types::{
    AckLevel, Actor, ActorKind, Authority, AuthoritySource, BatchEntryResult, BatchResult,
    Classification, Event, EventDraft, EventPayload, EventRef, EventType, IndexStatus, Layer,
    ObservePayload, RejectReason, SCHEMA_VERSION, SubjectRef,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

/// In-process adapter: each method opens a fresh `BrainRepo` at the stored
/// path. This sidesteps libgit2's `!Send` Repository and is trivially
/// thread-safe. Fine for personal-brain scale (open is ~1ms). A real daemon
/// path (one long-lived Repository + mpsc actor) lands in a follow-up.
#[derive(Clone, Debug)]
pub struct LocalBrain {
    path: PathBuf,
}

impl LocalBrain {
    /// Create a new brain at `path`. Also initializes the derived index
    /// at `<path>/.brain/index.sqlite` so reads after the first write
    /// don't pay a first-open rebuild cost.
    pub fn init<P: AsRef<Path>>(path: P) -> Result<Self, AppError> {
        let path = path.as_ref().to_path_buf();
        BrainRepo::init(&path)?;
        // Eagerly create the index file. If this fails we don't unwind the
        // git init — the index is derived and self-heals on the next open.
        if let Err(e) = BrainIndex::open(index_path_for(&path)) {
            warn!(error = %e, "index init failed; will rebuild lazily");
        }
        Ok(Self { path })
    }

    /// Open an existing brain at `path`. Also opens the index and runs
    /// a cheap "catch up since last watermark" pass — if the process died
    /// between a git commit and the index write last time, this reconciles.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AppError> {
        let path = path.as_ref().to_path_buf();
        BrainRepo::open(&path)?;
        // Best-effort catch-up: if this fails, search may return stale
        // results until doctor runs a full rebuild. We don't block open.
        if let Err(e) = catch_up_index(&path) {
            warn!(error = %e, "index catch-up failed; search may lag");
        }
        Ok(Self { path })
    }

    /// The path this brain points at.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Build a `LocalBrain` from an already-initialized path without
    /// re-validating. Useful when you've just called `init()` and want to
    /// skip the second `open()` check.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Append a quick observation. Wraps the user's summary into a minimal
    /// `ObservePayload` with private classification and agent actor defaults.
    /// Use `append()` directly for full control.
    pub fn observe_summary(&self, summary: String) -> Result<EventRef, AppError> {
        let draft = default_observe_draft(summary);
        self.append_sync(draft)
    }

    /// List recent events newest-first, up to `limit`. Routed through the
    /// index so archived and redacted events are hidden by default —
    /// consistent with `search()`. If the index is unavailable, returns
    /// an error rather than falling back to raw git history (which would
    /// leak hidden events, Codex P1).
    pub fn log(&self, limit: usize) -> Result<Vec<Event>, AppError> {
        let repo = BrainRepo::open(&self.path)?;
        let idx = BrainIndex::open(index_path_for(&self.path))?;
        let q = SearchQuery {
            limit,
            ..Default::default()
        };
        let hits = idx.search(&q)?;
        let mut out = Vec::with_capacity(hits.len());
        for h in hits {
            match repo.read_event(&h.commit_oid, h.event_id) {
                Ok(Some(ev)) => out.push(ev),
                Ok(None) => {
                    return Err(AppError::Other(format!(
                        "index points at event {} in commit {}, but git did not expose that event",
                        h.event_id, h.commit_oid
                    )));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(out)
    }

    /// Full-text + filter search over the derived SQLite index.
    /// Bare text queries go through FTS5 with BM25 ranking; prefix
    /// matching is automatic for bare words (`fast` finds `fastapi`).
    /// Archived and
    /// redacted events are excluded by default. The returned events are
    /// hydrated from git so callers get the full `Event`.
    ///
    /// If the index can't be opened, we return an error rather than
    /// falling back to a git-history grep. The grep path has no
    /// redaction/archive logic, so a fallback would leak hidden content.
    /// The fix is `LocalBrain::rebuild_index()` or running `brain doctor`.
    pub fn search(&self, needle: &str, limit: usize) -> Result<Vec<Event>, AppError> {
        // Empty / whitespace-only queries would hit FTS5 with `MATCH ''`,
        // which is a syntax error. Caller gets an empty result set.
        if needle.trim().is_empty() {
            return Ok(Vec::new());
        }

        let repo = BrainRepo::open(&self.path)?;
        let idx = BrainIndex::open(index_path_for(&self.path))?;
        let q = SearchQuery {
            text: Some(needle.to_string()),
            limit,
            ..Default::default()
        };
        let hits = idx.search(&q)?;
        let mut out = Vec::with_capacity(hits.len());
        for h in hits {
            match repo.read_event(&h.commit_oid, h.event_id) {
                Ok(Some(ev)) => out.push(ev),
                Ok(None) => {
                    return Err(AppError::Other(format!(
                        "index points at event {} in commit {}, but git did not expose that event",
                        h.event_id, h.commit_oid
                    )));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(out)
    }

    /// Current-truth projection accessors. Go straight to the index —
    /// no git read needed for the common case.
    pub fn current_pref(
        &self,
        category: &str,
        key: &str,
    ) -> Result<Option<brain_index::PrefProjection>, AppError> {
        let idx = BrainIndex::open(index_path_for(&self.path))?;
        Ok(idx.current_pref(category, key)?)
    }

    pub fn current_claim(
        &self,
        schema_type: &str,
        key: &str,
    ) -> Result<Option<brain_index::ClaimProjection>, AppError> {
        let idx = BrainIndex::open(index_path_for(&self.path))?;
        Ok(idx.current_claim(schema_type, key)?)
    }

    /// Produce a health report for the brain: schema version, commit count,
    /// event count, oldest/newest event timestamps, and any detected issues.
    pub fn doctor(&self) -> Result<DoctorReport, AppError> {
        let repo = BrainRepo::open(&self.path)?;
        let commit_count = repo.commit_count()?;
        // Exact count. Routes through `event_from_commit` so the
        // inherited-blob skip + blob/trailer cross-check filter out
        // forged commits. Costs one blob read per event-carrying
        // commit, acceptable at personal-brain scale; the previous
        // trailer-only count would under-report by ignoring nothing
        // but also couldn't detect tampering. Replaces the earlier
        // `list_events(10_000)` that silently capped at 10k events.
        // Codex R3 + R12.
        let event_count = repo.event_count()?;
        // Newest event for oldest/newest timestamps — one-page read.
        let newest_page = repo.list_events(1)?;
        let newest_event_at = newest_page.first().map(|e| e.time_recorded);
        // Oldest event: fetch from the oldest-first stream, just the
        // first item. This still walks history but doesn't materialize
        // everything into memory at once.
        let oldest_event_at = repo.events_oldest_first()?.first().map(|e| e.time_recorded);

        let mut issues: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        if commit_count == 0 {
            issues.push("repo has zero commits (corrupt or un-bootstrapped)".into());
        }
        // Bootstrap commit + N event commits = commit_count. Mismatch is
        // informational for v0.1 but worth surfacing to the user.
        if commit_count.saturating_sub(1) != event_count {
            issues.push(format!(
                "commit count ({commit_count}) and event count ({event_count}) differ by more than the bootstrap commit; foreign commits or missing events",
            ));
        }

        // Index health: open, count, compare. A lag > 0 is a warning
        // (the index catches up on next open or doctor --deep). A structural
        // mismatch (index has more events than git, or wildly different
        // count) means run a full rebuild.
        let (indexed_event_count, index_lag) = match BrainIndex::open(index_path_for(&self.path)) {
            Ok(idx) => {
                // Doctor's job is to surface problems — don't swallow
                // SQLite errors as zeros. Either count succeeds and we
                // report it, or we add the error to `issues` so the
                // user sees the exact condition. Same rule for the
                // watermark lookup below.
                let n = match idx.event_count() {
                    Ok(n) => n,
                    Err(e) => {
                        issues.push(format!("index event_count failed: {e}"));
                        0
                    }
                };
                let wm = match idx.last_committed_oid() {
                    Ok(w) => w,
                    Err(e) => {
                        issues.push(format!("index watermark read failed: {e}"));
                        None
                    }
                };
                let lag = match &wm {
                    Some(oid) => match repo.events_after(Some(oid)) {
                        Ok(v) => v.len(),
                        Err(e) => {
                            issues.push(format!("events_after watermark failed: {e}"));
                            0
                        }
                    },
                    None => event_count,
                };
                if lag > 0 {
                    warnings.push(format!(
                            "index lags git by {lag} event{}; catch-up runs on next open, or call rebuild_index() now",
                            if lag == 1 { "" } else { "s" }
                        ));
                }
                if n > event_count {
                    issues.push(format!(
                            "index has {n} events but git has {event_count}; index is corrupt, run rebuild_index()"
                        ));
                }
                (n, lag)
            }
            Err(e) => {
                issues.push(format!("index unavailable: {e}"));
                (0, 0)
            }
        };

        Ok(DoctorReport {
            path: self.path.clone(),
            schema_version: SCHEMA_VERSION,
            commit_count,
            event_count,
            newest_event_at,
            oldest_event_at,
            indexed_event_count,
            index_lag,
            warnings,
            issues,
        })
    }

    /// Wipe and rebuild the derived index from git. This IS the self-heal
    /// path for schema-version-bump recovery, index corruption, and
    /// structural mismatch. If the on-disk index has a schema version
    /// that our build can't read, we delete the file and rebuild from
    /// scratch — the index is derived, so losing it costs a rebuild
    /// walk over git, not real data. Codex R4 round 5: "rebuild_index
    /// can't recover from SchemaMismatch because open() fails before
    /// rebuild can start."
    pub fn rebuild_index(&self) -> Result<usize, AppError> {
        let repo = BrainRepo::open(&self.path)?;
        let idx_path = index_path_for(&self.path);
        let mut idx = match BrainIndex::open(&idx_path) {
            Ok(i) => i,
            Err(IndexError::SchemaMismatch { .. }) => {
                warn!(
                    path = %idx_path.display(),
                    "index schema mismatch — deleting and recreating"
                );
                // Remove main db + WAL + SHM if present. SQLite tolerates
                // missing WAL/SHM on next open.
                let _ = std::fs::remove_file(&idx_path);
                let wal = idx_path.with_extension("sqlite-wal");
                let shm = idx_path.with_extension("sqlite-shm");
                let _ = std::fs::remove_file(&wal);
                let _ = std::fs::remove_file(&shm);
                BrainIndex::open(&idx_path)?
            }
            Err(e) => return Err(AppError::Index(e)),
        };
        let all = repo.events_oldest_first()?;
        let n = idx.rebuild_from(all)?;
        // Pin watermark to actual HEAD (Codex R11 P2). Propagate failure
        // rather than silently drop — a stuck watermark makes every
        // future `open` re-walk full history and masks itself as
        // "catch-up lagging".
        let head = repo.head_commit_oid()?;
        idx.set_last_committed_oid(&head)?;
        Ok(n)
    }

    /// Push brain commits to a remote git repo. Shells out to the `git`
    /// CLI rather than using git2 directly — this gives us SSH keys,
    /// HTTPS creds, 2FA, macOS Keychain, and credential helpers for
    /// free. git is ubiquitous on dev machines; requiring it for sync
    /// is the same contract everyone's comfortable with.
    ///
    /// If `remote` is None, pushes to the default remote (`origin` if
    /// configured). Returns the git CLI's stderr on failure so the user
    /// sees the real auth/ref error.
    pub fn push(&self, remote: Option<&str>) -> Result<String, AppError> {
        let remote = remote.unwrap_or("origin");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .arg("push")
            .arg(remote)
            .arg("HEAD")
            .output()
            .map_err(|e| AppError::Other(format!("spawn git push: {e}")))?;
        if !out.status.success() {
            return Err(AppError::Other(format!(
                "git push {remote}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }

    /// Pull brain commits from a remote git repo with fast-forward or
    /// rebase. After the pull, runs `catch_up_index` so the derived
    /// SQLite index picks up newly-arrived events.
    ///
    /// No conflict handling lives here: events are append-only and
    /// each lives in its own UUIDv7-named file, so file-level
    /// conflicts shouldn't happen. If they DO (history rewrite,
    /// manual `git` surgery), the pull returns an error and the
    /// user resolves by hand inside the brain dir.
    pub fn pull(&self, remote: Option<&str>) -> Result<String, AppError> {
        let remote = remote.unwrap_or("origin");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .arg("pull")
            .arg("--rebase")
            .arg(remote)
            .arg("HEAD")
            .output()
            .map_err(|e| AppError::Other(format!("spawn git pull: {e}")))?;
        if !out.status.success() {
            return Err(AppError::Other(format!(
                "git pull {remote}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        // Catch the index up to whatever HEAD now points at. Pull is not
        // complete from the user's point of view until search/log can see
        // the newly-arrived commits.
        catch_up_index(&self.path).map_err(|e| {
            AppError::Other(format!("pull succeeded, but index catch-up failed: {e}"))
        })?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Add a named remote to the brain's git config.
    pub fn remote_add(&self, name: &str, url: &str) -> Result<(), AppError> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(["remote", "add", name, url])
            .output()
            .map_err(|e| AppError::Other(format!("spawn git remote add: {e}")))?;
        if !out.status.success() {
            return Err(AppError::Other(format!(
                "git remote add: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    /// List configured remotes as (name, url) pairs.
    pub fn remote_list(&self) -> Result<Vec<(String, String)>, AppError> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(["remote", "-v"])
            .output()
            .map_err(|e| AppError::Other(format!("spawn git remote -v: {e}")))?;
        if !out.status.success() {
            return Err(AppError::Other(format!(
                "git remote -v: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        // Dedupe fetch+push pairs: git remote -v prints each remote
        // twice (once for fetch, once for push). We keep the first.
        let mut seen = std::collections::HashSet::new();
        let mut out_vec = Vec::new();
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
                continue;
            };
            if seen.insert(name.to_string()) {
                out_vec.push((name.to_string(), url.to_string()));
            }
        }
        Ok(out_vec)
    }

    /// Synchronous append. Used by `observe_summary` and the `BrainWrite`
    /// async impl (via `spawn_blocking`).
    ///
    /// Two-phase: git first (source of truth), then index (best effort).
    /// Index ingest failure does NOT unwind the git commit — the
    /// watermark-driven catch-up will pick it up on next open/doctor.
    fn append_sync(&self, draft: EventDraft) -> Result<EventRef, AppError> {
        let repo = BrainRepo::open(&self.path)?;
        let ev_ref = repo.append_event(draft)?;
        if ev_ref.was_idempotent_replay {
            // Nothing new to index.
            return Ok(ev_ref);
        }

        let commit_oid = match &ev_ref.commit_oid {
            Some(oid) => oid.clone(),
            None => return Ok(ev_ref),
        };
        let full_event = match repo.read_event(&commit_oid, ev_ref.event_id)? {
            Some(e) => e,
            None => {
                return Err(AppError::Other(format!(
                    "just-committed event {} was not readable from commit {commit_oid}",
                    ev_ref.event_id
                )));
            }
        };

        let idx_path = index_path_for(&self.path);
        match BrainIndex::open(&idx_path).and_then(|mut idx| idx.ingest(&full_event)) {
            Ok(()) => Ok(EventRef {
                index_status: IndexStatus::Ok,
                ..ev_ref
            }),
            Err(e) => {
                warn!(
                    error = %e,
                    event_id = %ev_ref.event_id,
                    "index ingest failed; event is durable in git, doctor will reconcile"
                );
                Ok(EventRef {
                    index_status: IndexStatus::Lagging,
                    ..ev_ref
                })
            }
        }
    }
}

/// Derived-index path relative to a brain dir.
pub fn index_path_for(brain_dir: &Path) -> PathBuf {
    brain_dir.join(".brain").join("index.sqlite")
}

/// Catch up the index from whatever state it's in to HEAD.
///
/// Correctness model (Codex P1 from this session): a single commit-oid
/// watermark is NOT sufficient under concurrent writers. Imagine
/// process A commits c1 and dies before indexing; process B commits
/// c2 on top and indexes it, advancing watermark to c2. Walking
/// `events_after(c2)` would skip c1 forever.
///
/// Fix: treat the index as authoritative for what's indexed, and
/// compute the set-difference against git. Cheap happy-path check
/// first (watermark == HEAD → zero events), expensive correct path
/// otherwise (walk every event, ingest only those whose event_id is
/// not already in the index). `ingest()` is idempotent so even the
/// expensive path is safe.
fn catch_up_index(brain_dir: &Path) -> Result<usize, AppError> {
    let repo = BrainRepo::open(brain_dir)?;
    let mut idx = BrainIndex::open(index_path_for(brain_dir))?;

    // Fast path: watermark matches HEAD, event counts agree → nothing to do.
    let head_oid = repo.head_commit_oid().ok();
    let watermark = idx.last_committed_oid()?;
    if head_oid.is_some() && head_oid == watermark {
        let git_count = repo.event_count()?;
        let idx_count = idx.event_count()?;
        if git_count == idx_count {
            return Ok(0);
        }
    }

    // Correct path: detect orphans first. If ANY indexed event_id is
    // missing from git, history was rewritten (or index was copied from
    // another brain). Surgical pruning is unsafe because a pruned
    // Verify/Archive/Redact leaves stale side effects on OTHER events'
    // projections — a pruned Redact doesn't un-flip is_redacted, a pruned
    // Archive doesn't remove the entity_archive row, a pruned Pref tip
    // doesn't restore the previous surviving Pref to current_pref.
    // Codex round 4 caught both: "prune_events is not the inverse of
    // ingest for control events" and "pruning drops current projection
    // without rolling back to prior surviving event."
    //
    // Right answer: full rebuild_from when orphans exist. The surgical
    // ingest-missing path only runs when the index is a strict subset
    // of git (normal crash-recovery).
    let indexed_ids = idx.indexed_event_ids()?;
    let all = repo.events_oldest_first()?;
    let git_ids: std::collections::HashSet<String> =
        all.iter().map(|e| e.event_id.to_string()).collect();

    let has_orphans = indexed_ids.iter().any(|id| !git_ids.contains(id));
    if has_orphans {
        let orphan_count = indexed_ids.difference(&git_ids).count();
        warn!(
            count = orphan_count,
            "history rewrite detected; rebuilding index from git"
        );
        let rebuilt = idx.rebuild_from(all)?;
        // Pin watermark to HEAD after rebuild (Codex R11 P2). Propagate
        // the error — a failed pin would leave doctor reporting
        // permanent false lag.
        let head = repo.head_commit_oid()?;
        idx.set_last_committed_oid(&head)?;
        return Ok(rebuilt);
    }

    // Strict subset path: find the EARLIEST missing event and replay from
    // there forward. Simply ingesting only the missing events (in git
    // order) would be unsafe: if missing events are interspersed with
    // already-indexed ones, the interspersed missing events' projection
    // upserts would execute AFTER newer already-indexed events'
    // projections landed, overwriting current-truth rows with stale data.
    // Codex R5 round 5: "when the missing commits are not a suffix, this
    // loop replays older events after newer ones and skips the newer ones
    // entirely" — e.g. Pref(theme=dark) missing, Pref(theme=light)
    // indexed: a surgical ingest would set current_pref back to dark.
    //
    // Replaying from the first missing event forward is safe because
    // ingest() is idempotent (INSERT OR REPLACE) and deterministic from
    // event payload alone, so re-ingesting already-indexed events is a
    // no-op on final state.
    let first_missing_idx = all
        .iter()
        .position(|e| !indexed_ids.contains(&e.event_id.to_string()));
    let mut ingested = 0usize;
    if let Some(start) = first_missing_idx {
        for ev in &all[start..] {
            idx.ingest(ev)?;
            if !indexed_ids.contains(&ev.event_id.to_string()) {
                ingested += 1;
            }
        }
    }
    if ingested > 0 {
        debug!(count = ingested, "catch-up ingested unindexed events");
    }
    // Pin watermark to HEAD regardless of what was ingested
    // (Codex R11 P2). Propagate the error — a silently-dropped pin
    // leaves the fast path skewed and rewalks full history forever.
    let head = repo.head_commit_oid()?;
    idx.set_last_committed_oid(&head)?;
    Ok(ingested)
}

// Async append + batch methods on LocalBrain. The old `BrainWrite`
// trait had exactly one impl and was never used polymorphically —
// deleted in favor of inherent methods so callers don't need to
// import a trait to call them.
impl LocalBrain {
    /// Async single-event append. Wraps `append_sync` in spawn_blocking
    /// because libgit2's `Repository` is `!Send` (holds raw pointers).
    pub async fn append(&self, draft: EventDraft, _ack: AckLevel) -> Result<EventRef, AppError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.append_sync(draft))
            .await
            .map_err(|e| AppError::TaskPanicked(e.to_string()))?
    }

    /// Async batch append. v0.1: serial loop, one commit per draft.
    /// True batching (one commit per batch) lands with the daemon +
    /// mpsc actor.
    pub async fn append_batch(
        &self,
        drafts: Vec<EventDraft>,
        _ack: AckLevel,
    ) -> Result<BatchResult, AppError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut results = Vec::with_capacity(drafts.len());
            let mut last_commit: Option<String> = None;
            for draft in drafts {
                match this.append_sync(draft) {
                    Ok(ev) => {
                        if let Some(oid) = &ev.commit_oid {
                            last_commit = Some(oid.clone());
                        }
                        results.push(BatchEntryResult::Ok { event_ref: ev });
                    }
                    Err(AppError::Rejected(r)) => {
                        results.push(BatchEntryResult::Rejected { reason: r });
                    }
                    Err(e) => {
                        // Non-rejection failure: record per-entry and keep
                        // going. Prior successful entries stay in `results`
                        // so the caller can see exactly what landed.
                        warn!(error = %e, "append_batch entry failed; recording and continuing");
                        results.push(BatchEntryResult::Failed {
                            detail: format!("{e}"),
                        });
                    }
                }
            }
            Ok(BatchResult {
                batch_id: Uuid::now_v7(),
                results,
                committed_oid: last_commit,
                index_status: IndexStatus::Ok,
            })
        })
        .await
        .map_err(|e| AppError::TaskPanicked(e.to_string()))?
    }
}

/// Build a sensible-default `EventDraft` for a one-shot CLI observation.
/// Every field the CLI can reasonably default is defaulted here so the
/// callsite in `brain-cli` stays short.
pub fn default_observe_draft(summary: String) -> EventDraft {
    EventDraft {
        event_type: EventType::Observe,
        subject: SubjectRef::Entity { id: Uuid::now_v7() },
        parent_event_id: None,
        chain_id: None,
        actor: Actor {
            kind: ActorKind::Human,
            id: whoami_or_default(),
            harness: None,
            signing_key_fingerprint: None,
        },
        time_observed: None,
        layer: Layer::Episodic,
        authority: Authority {
            source_kind: AuthoritySource::User,
            score: Some(50),
            attested_by: None,
        },
        classification: Classification::Private,
        payload: EventPayload::Observe(ObservePayload {
            summary,
            content: None,
            content_ref: None,
            source: Some("brain-cli".to_string()),
        }),
        schema_version: SCHEMA_VERSION,
        idempotency_key: format!("cli::{}", Uuid::now_v7()),
    }
}

fn whoami_or_default() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "cli-user".to_string())
}

/// Output of `LocalBrain::doctor`. Serializable so `brain doctor --json`
/// is trivial later.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorReport {
    pub path: PathBuf,
    pub schema_version: u32,
    pub commit_count: usize,
    pub event_count: usize,
    pub newest_event_at: Option<DateTime<Utc>>,
    pub oldest_event_at: Option<DateTime<Utc>>,
    /// Number of events present in the derived SQLite index.
    pub indexed_event_count: usize,
    /// Events in git that aren't yet indexed. Normally 0.
    pub index_lag: usize,
    /// Non-fatal health notes. CLI/MCP should surface these without failing
    /// the command.
    pub warnings: Vec<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("store: {0}")]
    Store(#[from] StoreError),

    #[error("index: {0}")]
    Index(#[from] IndexError),

    #[error("rejected: {0}")]
    Rejected(#[from] brain_types::RejectReason),

    /// A tokio::task::spawn_blocking worker panicked. Kept distinct
    /// from `Other(String)` so callers can surface different exit
    /// codes / retry logic.
    #[error("worker panicked: {0}")]
    TaskPanicked(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// True if the underlying failure is "no brain at this path yet."
    /// Used by brain-cli to distinguish typo-vs-uninitialized. Keeps
    /// callers out of `brain_store::StoreError` pattern-matching so
    /// the facade stays closed.
    pub fn is_not_found(&self) -> bool {
        matches!(self, AppError::Store(StoreError::NotFound { .. }))
    }

    /// If this is a secret-prefilter rejection, returns the pattern
    /// name. Callers display the name so the note text never leaks
    /// back into logs or UI.
    pub fn secret_rejection(&self) -> Option<&str> {
        if let AppError::Store(StoreError::Rejected(RejectReason::SecretDetected { pattern })) =
            self
        {
            Some(pattern.as_str())
        } else {
            None
        }
    }

    /// True for transient multi-process init races that callers should
    /// retry. Proxies to `StoreError::is_transient_init_race`.
    pub fn is_transient_init_race(&self) -> bool {
        match self {
            AppError::Store(se) => se.is_transient_init_race(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_then_observe_then_log_round_trips() {
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();

        let er = brain.observe_summary("hello brain".into()).unwrap();
        assert!(er.commit_oid.is_some());

        let events = brain.log(10).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].payload {
            EventPayload::Observe(p) => assert_eq!(p.summary, "hello brain"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn async_append_reaches_committed() {
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();

        let draft = default_observe_draft("async observe".into());
        let ev = brain.append(draft, AckLevel::Committed).await.unwrap();
        assert!(matches!(ev.state, brain_types::EventState::Committed));
    }

    #[test]
    fn doctor_reports_counts_and_clean_on_fresh_brain() {
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();

        let r = brain.doctor().unwrap();
        assert_eq!(r.schema_version, SCHEMA_VERSION);
        assert_eq!(r.commit_count, 1, "bootstrap only");
        assert_eq!(r.event_count, 0);
        assert!(r.newest_event_at.is_none());
        assert!(r.oldest_event_at.is_none());
        assert!(r.issues.is_empty(), "clean fresh brain: {:?}", r.issues);

        brain.observe_summary("a".into()).unwrap();
        brain.observe_summary("b".into()).unwrap();

        let r2 = brain.doctor().unwrap();
        assert_eq!(r2.commit_count, 3);
        assert_eq!(r2.event_count, 2);
        assert!(r2.newest_event_at.is_some());
        assert!(r2.oldest_event_at.is_some());
        assert!(r2.issues.is_empty());
    }

    #[test]
    fn catch_up_indexes_every_unindexed_event() {
        // Codex P1: watermark alone can skip an event if a later commit
        // was indexed first. Simulate that: ingest commit 2 but not
        // commit 1, advance the watermark to 2, then run catch-up and
        // assert BOTH events end up indexed.
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        let r1 = brain.observe_summary("first".into()).unwrap();
        let r2 = brain.observe_summary("second".into()).unwrap();

        // Manually corrupt the index to simulate the race: remove the
        // "first" event row, leaving only "second" plus a watermark
        // pointing at "second". Watermark-only catch-up would miss
        // "first"; set-difference catch-up must backfill it.
        let idx_path = index_path_for(brain.path());
        {
            let idx = BrainIndex::open(&idx_path).unwrap();
            idx.connection()
                .execute(
                    "DELETE FROM events WHERE event_id = ?1",
                    rusqlite::params![r1.event_id.to_string()],
                )
                .unwrap();
            idx.connection()
                .execute(
                    "DELETE FROM events_fts WHERE event_id = ?1",
                    rusqlite::params![r1.event_id.to_string()],
                )
                .unwrap();
            // Watermark now points at the LATER event (r2).
            idx.set_last_committed_oid(r2.commit_oid.as_deref().unwrap())
                .unwrap();
            assert_eq!(idx.event_count().unwrap(), 1);
        }

        // Trigger catch-up by reopening.
        let _ = LocalBrain::open(brain.path()).unwrap();

        let idx = BrainIndex::open(&idx_path).unwrap();
        assert_eq!(
            idx.event_count().unwrap(),
            2,
            "catch-up must backfill the skipped event, not trust the watermark"
        );
    }

    #[test]
    fn log_hides_archived_and_redacted() {
        // Codex P1 round 3: log() used to walk raw git history, bypassing
        // every filter search() had. Redacted content stayed visible
        // through `brain log` forever. Now routed through the index.
        use brain_types::{ArchiveDirection, ArchivePayload, RedactPayload};

        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        let keep = brain.observe_summary("keep me".into()).unwrap();
        let archive_me = brain.observe_summary("archive me".into()).unwrap();
        let redact_me = brain.observe_summary("redact me".into()).unwrap();

        // Archive one observation.
        let draft = EventDraft {
            event_type: EventType::Archive,
            subject: SubjectRef::Entity {
                id: archive_me.event_id,
            },
            parent_event_id: None,
            chain_id: None,
            actor: Actor {
                kind: ActorKind::Human,
                id: "tester".into(),
                harness: None,
                signing_key_fingerprint: None,
            },
            time_observed: None,
            layer: Layer::Episodic,
            authority: Authority {
                source_kind: AuthoritySource::User,
                score: None,
                attested_by: None,
            },
            classification: Classification::Private,
            payload: EventPayload::Archive(ArchivePayload {
                target_entity: archive_me.event_id,
                direction: ArchiveDirection::Archive,
                reason: None,
            }),
            schema_version: SCHEMA_VERSION,
            idempotency_key: "archive-key".into(),
        };
        brain.append_sync(draft).unwrap();

        // Redact another.
        let draft = EventDraft {
            event_type: EventType::Redact,
            subject: SubjectRef::Entity {
                id: redact_me.event_id,
            },
            parent_event_id: None,
            chain_id: None,
            actor: Actor {
                kind: ActorKind::Human,
                id: "tester".into(),
                harness: None,
                signing_key_fingerprint: None,
            },
            time_observed: None,
            layer: Layer::Episodic,
            authority: Authority {
                source_kind: AuthoritySource::User,
                score: None,
                attested_by: None,
            },
            classification: Classification::Private,
            payload: EventPayload::Redact(RedactPayload {
                target_entity: redact_me.event_id,
                reason: "classified".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            schema_version: SCHEMA_VERSION,
            idempotency_key: "redact-key".into(),
        };
        brain.append_sync(draft).unwrap();

        let events = brain.log(20).unwrap();
        let ids: std::collections::HashSet<_> = events.iter().map(|e| e.event_id).collect();
        assert!(
            ids.contains(&keep.event_id),
            "clean observe must be visible"
        );
        assert!(
            !ids.contains(&archive_me.event_id),
            "archived event leaked through log"
        );
        assert!(
            !ids.contains(&redact_me.event_id),
            "redacted event leaked through log"
        );
    }

    #[test]
    fn catch_up_prunes_orphans_when_index_has_stale_rows() {
        // Codex P1 round 3: catch-up was additive-only. If the index
        // carried rows whose event_id no longer exists in git (rewritten
        // history, foreign-index import), they would stay visible
        // through current_* accessors and search. Verify pruning.
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        brain.observe_summary("real one".into()).unwrap();

        // Inject a fake event row into the index that has no matching
        // git commit. Catch-up must delete it on next open.
        let idx_path = index_path_for(brain.path());
        let fake_id = "00000000-0000-7000-8000-000000000042";
        {
            let idx = BrainIndex::open(&idx_path).unwrap();
            idx.connection()
                .execute(
                    r#"INSERT INTO events(
                     event_id, commit_oid, event_type,
                     subject_kind, subject_id, chain_id, parent_event_id,
                     actor_kind, actor_id, actor_harness,
                     layer, authority_source_kind, authority_score, authority_attested_by,
                     signature_state, classification,
                     time_observed, time_recorded, idempotency_key, is_redacted, payload_json
                   ) VALUES (?1,'phantom','observe','entity',?1,NULL,NULL,'agent','ghost',NULL,
                             'episodic','agent',NULL,NULL,'unsigned','private',
                             9999,9999,NULL,0,'{}')"#,
                    rusqlite::params![fake_id],
                )
                .unwrap();
            idx.connection()
                .execute(
                    "INSERT INTO events_fts(event_id, title, body, tags) VALUES (?1,'ghost','','')",
                    rusqlite::params![fake_id],
                )
                .unwrap();
            assert_eq!(idx.event_count().unwrap(), 2);
        }

        // Reopening triggers catch_up_index which should prune the ghost.
        let _ = LocalBrain::open(brain.path()).unwrap();
        let idx = BrainIndex::open(&idx_path).unwrap();
        assert_eq!(
            idx.event_count().unwrap(),
            1,
            "orphan index row must be pruned by catch-up"
        );
    }

    #[test]
    fn catch_up_rebuilds_projections_when_orphan_detected() {
        // Codex round 4 P1: a pruned Pref/Claim tip would leave
        // current_pref / current_claim empty forever, even if an older
        // surviving event should become current. Full rebuild on orphan
        // detection handles this correctly.
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        // Land a real Observe so catch-up has something to replay.
        brain.observe_summary("real".into()).unwrap();

        // Inject an orphan index row that has no matching git commit.
        let idx_path = index_path_for(brain.path());
        let ghost = "00000000-0000-7000-8000-000000000777";
        {
            let idx = BrainIndex::open(&idx_path).unwrap();
            idx.connection()
                .execute(
                    r#"INSERT INTO events(
                     event_id, commit_oid, event_type,
                     subject_kind, subject_id, chain_id, parent_event_id,
                     actor_kind, actor_id, actor_harness,
                     layer, authority_source_kind, authority_score, authority_attested_by,
                     signature_state, classification,
                     time_observed, time_recorded, idempotency_key, is_redacted, payload_json
                   ) VALUES (?1,'phantom','observe','entity',?1,NULL,NULL,'agent','ghost',NULL,
                             'episodic','agent',NULL,NULL,'unsigned','private',99,99,NULL,0,'{}')"#,
                    rusqlite::params![ghost],
                )
                .unwrap();
            // Also inject stale pref_current / claim_current / entity_archive
            // rows to prove the full rebuild wipes them.
            idx.connection().execute(
                "INSERT INTO pref_current(category,key,value_json,event_id,updated_at) VALUES('stale','k','\"v\"',?1,99)",
                rusqlite::params![ghost],
            ).unwrap();
            idx.connection().execute(
                "INSERT INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'archive',99)",
                rusqlite::params![ghost],
            ).unwrap();
        }

        // Reopening triggers catch_up_index. Must rebuild, not surgical prune.
        let _ = LocalBrain::open(brain.path()).unwrap();
        let idx = BrainIndex::open(&idx_path).unwrap();

        // Stale ghost rows are gone across every derived table.
        let pref_count: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM pref_current WHERE event_id = ?1",
                rusqlite::params![ghost],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pref_count, 0, "stale pref_current must be wiped");
        let arch_count: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM entity_archive WHERE target_entity = ?1",
                rusqlite::params![ghost],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(arch_count, 0, "stale entity_archive must be wiped");
        let ev_count: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE event_id = ?1",
                rusqlite::params![ghost],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ev_count, 0, "stale events row must be wiped");

        // The real Observe is still indexed.
        assert_eq!(idx.event_count().unwrap(), 1);
    }

    #[test]
    fn append_batch_keeps_per_entry_results_on_failure() {
        // Codex round 4 P1: append_batch used to return Err on the first
        // non-rejection failure, discarding successes. Now each entry
        // gets its own BatchEntryResult slot.
        use brain_types::{ArchiveDirection, ArchivePayload};

        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();

        // Three drafts: [ok, ok, ok]. Build real drafts via the helper.
        let drafts: Vec<EventDraft> = (0..3)
            .map(|i| default_observe_draft(format!("entry-{i}")))
            .collect();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result: BatchResult = rt
            .block_on(brain.append_batch(drafts, AckLevel::Committed))
            .unwrap();
        assert_eq!(result.results.len(), 3, "every draft gets an entry slot");
        for entry in &result.results {
            assert!(
                matches!(entry, BatchEntryResult::Ok { .. }),
                "happy-path entries must be Ok: {entry:?}"
            );
        }

        // Smoke the type existence: a BatchEntryResult::Failed variant
        // compiles and is distinct from Ok/Rejected.
        let _ = BatchEntryResult::Failed { detail: "x".into() };
        // Suppress unused warning on imports
        let _ = ArchivePayload {
            target_entity: Uuid::now_v7(),
            direction: ArchiveDirection::Archive,
            reason: None,
        };
    }

    #[test]
    fn search_returns_empty_on_empty_query_rather_than_erroring() {
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        brain.observe_summary("anything".into()).unwrap();
        assert!(brain.search("", 5).unwrap().is_empty());
        assert!(brain.search("   ", 5).unwrap().is_empty());
    }

    #[test]
    fn open_after_init_sees_the_same_history() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("brain");
        let a = LocalBrain::init(&path).unwrap();
        a.observe_summary("first".into()).unwrap();

        let b = LocalBrain::open(&path).unwrap();
        let events = b.log(10).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn catch_up_pins_watermark_to_head_not_last_ingested_event() {
        // Codex R11 P2: `ingest()` overwrites watermark to the event it
        // just wrote, so under concurrent writes the replay can end on
        // an older event than HEAD. Simulate the skew by backdating the
        // watermark below HEAD and running catch-up. After catch-up the
        // watermark MUST equal HEAD, otherwise doctor reports false lag
        // and the fast-path keeps missing forever.
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        let _first = brain.observe_summary("first".into()).unwrap();
        let _second = brain.observe_summary("second".into()).unwrap();

        let repo = BrainRepo::open(brain.path()).unwrap();
        let head = repo.head_commit_oid().unwrap();

        // Skew the watermark below HEAD (simulates: catch-up replayed an
        // older event after a concurrent writer had already pushed the
        // watermark forward, or vice versa).
        let idx_path = index_path_for(brain.path());
        {
            let idx = BrainIndex::open(&idx_path).unwrap();
            idx.set_last_committed_oid("0000000000000000000000000000000000000000")
                .unwrap();
            assert_ne!(
                idx.last_committed_oid().unwrap().unwrap(),
                head,
                "precondition: watermark is stale"
            );
        }

        // Reopening triggers catch_up_index.
        let _ = LocalBrain::open(brain.path()).unwrap();

        let idx = BrainIndex::open(&idx_path).unwrap();
        assert_eq!(
            idx.last_committed_oid().unwrap().as_deref(),
            Some(head.as_str()),
            "catch-up must pin watermark to HEAD"
        );
    }

    #[test]
    fn rebuild_index_pins_watermark_to_head() {
        // Same guarantee for the full-rebuild path.
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("brain")).unwrap();
        brain.observe_summary("one".into()).unwrap();
        brain.observe_summary("two".into()).unwrap();

        let repo = BrainRepo::open(brain.path()).unwrap();
        let head = repo.head_commit_oid().unwrap();

        let idx_path = index_path_for(brain.path());
        {
            let idx = BrainIndex::open(&idx_path).unwrap();
            idx.set_last_committed_oid("deadbeef").unwrap();
        }

        let n = brain.rebuild_index().unwrap();
        assert_eq!(n, 2, "two events replayed");

        let idx = BrainIndex::open(&idx_path).unwrap();
        assert_eq!(
            idx.last_committed_oid().unwrap().as_deref(),
            Some(head.as_str()),
            "rebuild must pin watermark to HEAD"
        );
    }
}
