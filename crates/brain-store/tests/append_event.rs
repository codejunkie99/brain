//! Integration test: init a brain in a tempdir, append events, read them
//! back via the commit oid. No stubs, real git2, real filesystem.

use brain_store::BrainRepo;
use brain_types::*;
use chrono::Utc;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn sample_draft(idem: &str, summary: &str) -> EventDraft {
    EventDraft {
        event_type: EventType::Observe,
        subject: SubjectRef::Entity { id: Uuid::now_v7() },
        parent_event_id: None,
        chain_id: None,
        actor: Actor {
            kind: ActorKind::Agent,
            id: "claude-opus-4-7".to_string(),
            harness: Some("claude-desktop".to_string()),
            signing_key_fingerprint: None,
        },
        time_observed: Some(Utc::now()),
        layer: Layer::Episodic,
        authority: Authority {
            source_kind: AuthoritySource::Agent,
            score: Some(50),
            attested_by: None,
        },
        classification: Classification::Private,
        payload: EventPayload::Observe(ObservePayload {
            summary: summary.to_string(),
            content: Some("body".to_string()),
            content_ref: None,
            source: None,
        }),
        schema_version: SCHEMA_VERSION,
        idempotency_key: idem.to_string(),
    }
}

#[test]
fn init_creates_a_brain_with_one_bootstrap_commit() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    assert_eq!(
        brain.commit_count().unwrap(),
        1,
        "expected bootstrap commit"
    );
}

#[test]
fn init_rejects_existing_brain() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("brain");
    BrainRepo::init(&path).unwrap();
    let err = BrainRepo::init(&path).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::AlreadyExists { .. }),
        "got: {err:?}"
    );
}

#[test]
fn open_fails_on_missing_brain() {
    let td = TempDir::new().unwrap();
    let err = BrainRepo::open(td.path().join("no-such-brain")).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::NotFound { .. }),
        "got: {err:?}"
    );
}

#[test]
fn append_event_creates_a_commit_and_is_readable_back() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    assert_eq!(brain.commit_count().unwrap(), 1);

    let event_ref = brain
        .append_event(sample_draft("test::append::1", "first observation"))
        .unwrap();

    assert!(matches!(event_ref.state, EventState::Committed));
    assert!(event_ref.commit_oid.is_some());
    assert_eq!(
        brain.commit_count().unwrap(),
        2,
        "expected bootstrap + append"
    );

    // Read back
    let commit_oid = event_ref.commit_oid.as_deref().unwrap();
    let event = brain
        .read_event(commit_oid, event_ref.event_id)
        .unwrap()
        .expect("event should be readable");

    assert_eq!(event.event_id, event_ref.event_id);
    assert_eq!(event.event_type, EventType::Observe);
    assert_eq!(
        event.commit_oid, commit_oid,
        "commit_oid enriched at read time"
    );
    assert_eq!(event.schema_version, SCHEMA_VERSION);
    assert_eq!(event.idempotency_key.as_deref(), Some("test::append::1"));
    match &event.payload {
        EventPayload::Observe(p) => assert_eq!(p.summary, "first observation"),
        other => panic!("wrong payload variant: {other:?}"),
    }
}

#[test]
fn appending_three_events_produces_three_commits_on_top_of_bootstrap() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    let a = brain.append_event(sample_draft("a", "A")).unwrap();
    let b = brain.append_event(sample_draft("b", "B")).unwrap();
    let c = brain.append_event(sample_draft("c", "C")).unwrap();

    assert_eq!(brain.commit_count().unwrap(), 4, "bootstrap + 3 appends");

    // All three readable via their own commits.
    for er in [&a, &b, &c] {
        let oid = er.commit_oid.as_deref().unwrap();
        let ev = brain.read_event(oid, er.event_id).unwrap().unwrap();
        assert_eq!(ev.event_id, er.event_id);
    }

    // The underlying git tree is cumulative, but `read_event` intentionally
    // exposes only the event introduced by the supplied commit so callers get
    // the same forgery defenses as list/count/idempotency.
    let c_oid = c.commit_oid.as_deref().unwrap();
    assert!(brain.read_event(c_oid, a.event_id).unwrap().is_none());
    assert!(brain.read_event(c_oid, b.event_id).unwrap().is_none());
}

#[test]
fn list_events_returns_newest_first() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    let a = brain.append_event(sample_draft("idem-a", "first")).unwrap();
    let b = brain
        .append_event(sample_draft("idem-b", "second"))
        .unwrap();
    let c = brain.append_event(sample_draft("idem-c", "third")).unwrap();

    let events = brain.list_events(10).unwrap();
    assert_eq!(events.len(), 3);
    // Newest first: c, b, a.
    assert_eq!(events[0].event_id, c.event_id);
    assert_eq!(events[1].event_id, b.event_id);
    assert_eq!(events[2].event_id, a.event_id);

    // commit_oid is enriched from the enclosing commit, not the JSON blob.
    for ev in &events {
        assert!(!ev.commit_oid.is_empty());
    }
}

#[test]
fn list_events_limits_result_count() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    for i in 0..5 {
        brain
            .append_event(sample_draft(&format!("idem-{i}"), &format!("e{i}")))
            .unwrap();
    }
    let limited = brain.list_events(2).unwrap();
    assert_eq!(limited.len(), 2);
}

#[test]
fn list_events_skips_bootstrap_commit() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    // Bootstrap commit exists but carries no event_id trailer.
    assert_eq!(brain.commit_count().unwrap(), 1);
    let events = brain.list_events(10).unwrap();
    assert_eq!(events.len(), 0, "bootstrap commit has no event");
}

#[test]
fn search_events_matches_on_summary_case_insensitive() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    brain
        .append_event(sample_draft("a", "FastAPI PKCE worked on first try"))
        .unwrap();
    brain
        .append_event(sample_draft("b", "rate limiter added to middleware"))
        .unwrap();
    brain
        .append_event(sample_draft("c", "upgraded to fastapi-users 14"))
        .unwrap();

    let hits = brain.search_events("fastapi", 10).unwrap();
    assert_eq!(hits.len(), 2, "expected 2 fastapi hits");

    let hits_mixed_case = brain.search_events("FASTAPI", 10).unwrap();
    assert_eq!(hits_mixed_case.len(), 2, "case-insensitive");

    let hits_none = brain.search_events("quantum", 10).unwrap();
    assert_eq!(hits_none.len(), 0);
}

#[test]
fn search_events_respects_limit() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    for i in 0..5 {
        brain
            .append_event(sample_draft(
                &format!("idem-{i}"),
                &format!("common keyword entry {i}"),
            ))
            .unwrap();
    }
    let hits = brain.search_events("common", 3).unwrap();
    assert_eq!(hits.len(), 3);
}

#[test]
fn duplicate_idempotency_key_returns_replay_no_new_commit() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    let first = brain.append_event(sample_draft("same-key", "v1")).unwrap();
    let commits_after_first = brain.commit_count().unwrap();
    assert_eq!(commits_after_first, 2);

    // Re-submit with identical key, different summary. Must return replay.
    let replay = brain
        .append_event(sample_draft("same-key", "v2-ignored"))
        .unwrap();

    assert!(replay.was_idempotent_replay);
    assert_eq!(
        replay.event_id, first.event_id,
        "replay should return prior event_id"
    );
    assert_eq!(replay.commit_oid, first.commit_oid);
    assert_eq!(
        brain.commit_count().unwrap(),
        commits_after_first,
        "replay must not create a new commit"
    );

    // The content of the first append wins.
    let ev = brain
        .read_event(first.commit_oid.as_deref().unwrap(), first.event_id)
        .unwrap()
        .unwrap();
    match &ev.payload {
        brain_types::EventPayload::Observe(p) => assert_eq!(p.summary, "v1"),
        other => panic!("wrong payload variant: {other:?}"),
    }
}

#[test]
fn concurrent_same_idempotency_key_lands_once() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("brain");
    BrainRepo::init(&path).unwrap();

    let path = Arc::new(path);
    let mut handles = Vec::new();
    for i in 0..16 {
        let path = Arc::clone(&path);
        handles.push(std::thread::spawn(move || {
            let brain = BrainRepo::open(&*path).unwrap();
            brain
                .append_event(sample_draft("same-concurrent-key", &format!("attempt {i}")))
                .unwrap()
        }));
    }

    let refs: Vec<EventRef> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first_id = refs[0].event_id;
    assert!(
        refs.iter().all(|er| er.event_id == first_id),
        "all concurrent replays should resolve to the same event_id"
    );

    let brain = BrainRepo::open(&*path).unwrap();
    assert_eq!(brain.event_count().unwrap(), 1);
    assert_eq!(
        brain.commit_count().unwrap(),
        2,
        "bootstrap + one event commit only"
    );
}

#[test]
fn fresh_idempotency_key_creates_distinct_event() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();

    let a = brain.append_event(sample_draft("key-a", "A")).unwrap();
    let b = brain.append_event(sample_draft("key-b", "B")).unwrap();

    assert_ne!(a.event_id, b.event_id);
    assert!(!a.was_idempotent_replay);
    assert!(!b.was_idempotent_replay);
}

#[test]
fn append_rejects_secret_without_landing_a_commit() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    let before = brain.commit_count().unwrap();

    // A plausible-looking anthropic-style key embedded in a note. The
    // prefilter must catch it BEFORE the commit lands, so the secret never
    // enters git history.
    let draft = sample_draft(
        "secret-test",
        "my key is sk-ant-abcdefghijklmnopqrstuvwxyz0123456789abcdef",
    );
    let err = brain.append_event(draft).unwrap_err();
    match err {
        brain_store::StoreError::Rejected(RejectReason::SecretDetected { pattern }) => {
            assert_eq!(pattern, "anthropic-key");
        }
        other => panic!("expected SecretDetected, got: {other:?}"),
    }

    assert_eq!(
        brain.commit_count().unwrap(),
        before,
        "secret-bearing draft must not land a commit"
    );
}

#[test]
fn open_refuses_a_non_brain_git_repo() {
    // A plain git repo without the SCHEMA marker. `brain --brain-dir ~/my-project`
    // must refuse to attach so we can't mutate the user's real project by writing
    // events/ into its history.
    let td = TempDir::new().unwrap();
    let repo_path = td.path().join("real-project");
    std::fs::create_dir_all(&repo_path).unwrap();
    git2::Repository::init(&repo_path).unwrap();

    let err = BrainRepo::open(&repo_path).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::NotABrain { .. }),
        "got: {err:?}"
    );
}

#[test]
fn open_refuses_a_bare_git_repo() {
    // Bare repos have no .git subdir — the old sentinel check missed them.
    // If treated as NotFound by the CLI write path, auto-init would nest a
    // brain INSIDE the bare repo's directory.
    let td = TempDir::new().unwrap();
    let bare = td.path().join("project.git");
    git2::Repository::init_bare(&bare).unwrap();

    let err = BrainRepo::open(&bare).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::NotABrain { .. }),
        "expected NotABrain for bare repo, got: {err:?}"
    );
}

#[test]
fn init_refuses_to_clobber_a_bare_git_repo() {
    // `brain --brain-dir /path/to/project.git note "x"` must NOT try to
    // init a new brain on top of a bare repo.
    let td = TempDir::new().unwrap();
    let bare = td.path().join("project.git");
    git2::Repository::init_bare(&bare).unwrap();

    let err = BrainRepo::init(&bare).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::AlreadyExists { .. }),
        "expected AlreadyExists for existing bare repo, got: {err:?}"
    );
}

#[test]
fn open_refuses_git_repo_whose_schema_is_a_directory() {
    // Some real repo could legitimately have a `SCHEMA/` directory in its
    // tree (SQL migrations, schema generators). We must NOT treat that as
    // a brain marker.
    let td = TempDir::new().unwrap();
    let repo_path = td.path().join("with-schema-dir");
    std::fs::create_dir_all(&repo_path).unwrap();
    let repo = git2::Repository::init(&repo_path).unwrap();

    // Build a tree that has SCHEMA as a subtree (directory), not a blob.
    let inner_blob = repo.blob(b"migration 001").unwrap();
    let mut schema_tb = repo.treebuilder(None).unwrap();
    schema_tb
        .insert("001.sql", inner_blob, git2::FileMode::Blob.into())
        .unwrap();
    let schema_tree = schema_tb.write().unwrap();

    let mut root_tb = repo.treebuilder(None).unwrap();
    root_tb
        .insert("SCHEMA", schema_tree, git2::FileMode::Tree.into())
        .unwrap();
    let root_tree_oid = root_tb.write().unwrap();
    let root_tree = repo.find_tree(root_tree_oid).unwrap();
    let sig = git2::Signature::now("x", "x@x").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &root_tree, &[])
        .unwrap();

    let err = BrainRepo::open(&repo_path).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::NotABrain { .. }),
        "expected NotABrain when SCHEMA is a directory, got: {err:?}"
    );
}

#[test]
fn open_refuses_git_repo_whose_schema_blob_is_garbage() {
    // A real repo that happens to have a file literally named SCHEMA but
    // whose content is not a brain marker. Must refuse.
    let td = TempDir::new().unwrap();
    let repo_path = td.path().join("with-schema-garbage");
    std::fs::create_dir_all(&repo_path).unwrap();
    let repo = git2::Repository::init(&repo_path).unwrap();

    let schema_blob = repo.blob(b"# Copyright 2024\n").unwrap();
    let empty_events = repo.treebuilder(None).unwrap().write().unwrap();
    let mut root_tb = repo.treebuilder(None).unwrap();
    root_tb
        .insert("SCHEMA", schema_blob, git2::FileMode::Blob.into())
        .unwrap();
    root_tb
        .insert("events", empty_events, git2::FileMode::Tree.into())
        .unwrap();
    let root_tree_oid = root_tb.write().unwrap();
    let root_tree = repo.find_tree(root_tree_oid).unwrap();
    let sig = git2::Signature::now("x", "x@x").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &root_tree, &[])
        .unwrap();

    let err = BrainRepo::open(&repo_path).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::NotABrain { .. }),
        "expected NotABrain for garbage SCHEMA blob, got: {err:?}"
    );
}

#[test]
fn open_refuses_an_unborn_git_repo() {
    // A freshly `git init`'d repo with no HEAD commit must not pass.
    let td = TempDir::new().unwrap();
    let repo_path = td.path().join("unborn");
    std::fs::create_dir_all(&repo_path).unwrap();
    git2::Repository::init(&repo_path).unwrap();

    let err = BrainRepo::open(&repo_path).unwrap_err();
    assert!(
        matches!(err, brain_store::StoreError::NotABrain { .. }),
        "expected NotABrain for unborn repo, got: {err:?}"
    );
}

#[test]
fn append_rejects_secret_in_nested_pref_value() {
    // The old handpicked-fields scan only covered summary/content/source on
    // Observe and a serialized blob of Claim/Pref `value`. But fields like
    // `Pref.previous_value` fell entirely outside the scan. Scanning the
    // full serialized Event JSON plugs this hole — verify.
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    let before = brain.commit_count().unwrap();

    let mut draft = sample_draft("nested-secret", "rotating api key");
    draft.event_type = EventType::Pref;
    draft.payload = EventPayload::Pref(PrefPayload {
        category: "auth".to_string(),
        key: "anthropic_api_key".to_string(),
        value: serde_json::json!("rotated"),
        previous_value: Some(serde_json::json!(
            "sk-ant-abcdefghijklmnopqrstuvwxyz0123456789abcdef"
        )),
    });

    let err = brain.append_event(draft).unwrap_err();
    match err {
        brain_store::StoreError::Rejected(RejectReason::SecretDetected { pattern }) => {
            assert_eq!(pattern, "anthropic-key");
        }
        other => panic!("expected SecretDetected for nested field, got: {other:?}"),
    }
    assert_eq!(brain.commit_count().unwrap(), before);
}

#[test]
fn append_rejects_event_exceeding_blob_size_cap() {
    // Codex round 4 P2: a successful oversized write would poison every
    // subsequent read because deserialize_event_blob caps at 1 MiB.
    // Align the cap at write time so "committed but unreadable" can't
    // happen.
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    let before = brain.commit_count().unwrap();

    // Build a payload whose serialized JSON will exceed 1 MiB.
    let huge = "A".repeat(1_100_000);
    let mut bad = sample_draft("size-cap", &huge);
    if let EventPayload::Observe(o) = &mut bad.payload {
        o.summary = huge;
    }

    let err = brain.append_event(bad).unwrap_err();
    match err {
        brain_store::StoreError::OversizedBlob { bytes, cap } => {
            assert!(bytes > cap, "oversized must exceed cap: {bytes} vs {cap}");
        }
        other => panic!("expected OversizedBlob, got: {other:?}"),
    }
    assert_eq!(
        brain.commit_count().unwrap(),
        before,
        "oversized draft must not land a commit"
    );
}

#[test]
fn open_rejects_detached_head() {
    // Codex R13 P2: a brain on detached HEAD silently loses commits the
    // next time someone switches branches. Refuse to open or append.
    use git2::Repository;
    let td = TempDir::new().unwrap();
    let path = td.path().join("brain");
    BrainRepo::init(&path).unwrap();

    // Detach HEAD by checking out the current commit sha directly.
    let repo = Repository::open(&path).unwrap();
    let head_oid = repo.head().unwrap().peel_to_commit().unwrap().id();
    repo.set_head_detached(head_oid).unwrap();
    drop(repo);

    let err = BrainRepo::open(&path).unwrap_err();
    match err {
        brain_store::StoreError::DetachedHead { .. } => {}
        other => panic!("expected DetachedHead, got {other:?}"),
    }
}

#[test]
fn append_rejects_runtime_detached_head() {
    // Agent-team review R14: the detached-HEAD check in
    // `append_event_once` (the second defense) had zero test coverage
    // — only `open()` was exercised. Force the runtime path by
    // detaching HEAD AFTER the brain is live and then calling append.
    use git2::Repository;
    let td = TempDir::new().unwrap();
    let path = td.path().join("brain");
    let brain = BrainRepo::init(&path).unwrap();

    // First append succeeds (HEAD still on a branch).
    brain
        .append_event(sample_draft("pre-detach", "before"))
        .unwrap();

    // Detach HEAD under the live brain.
    let repo = Repository::open(&path).unwrap();
    let head_oid = repo.head().unwrap().peel_to_commit().unwrap().id();
    repo.set_head_detached(head_oid).unwrap();
    drop(repo);

    // Second append must refuse rather than land a commit that
    // evaporates on the user's next branch checkout.
    let err = brain
        .append_event(sample_draft("post-detach", "after"))
        .unwrap_err();
    match err {
        brain_store::StoreError::DetachedHead { .. } => {}
        other => panic!("expected DetachedHead on append, got {other:?}"),
    }
}

#[test]
fn append_rejects_invalid_draft_without_mutating_repo() {
    let td = TempDir::new().unwrap();
    let brain = BrainRepo::init(td.path().join("brain")).unwrap();
    let before = brain.commit_count().unwrap();

    let mut bad = sample_draft("bad-mismatch", "x");
    bad.event_type = EventType::Claim; // doesn't match Observe payload

    let err = brain.append_event(bad).unwrap_err();
    match err {
        brain_store::StoreError::Rejected(RejectReason::TypeMismatch { .. }) => {}
        other => panic!("expected TypeMismatch, got: {other:?}"),
    }

    assert_eq!(
        brain.commit_count().unwrap(),
        before,
        "rejected draft must not land a commit"
    );
}

/// REGRESSION: an append must leave the working tree and the index in sync
/// with HEAD, so `git status` in a brain is clean.
///
/// Before the `materialize_event` fix, `append_event_once` wrote only to the
/// git object database (blob -> treebuilder -> commit) and never touched the
/// working tree or `.git/index`. The blob existed in HEAD but not on disk and
/// not in the index, so git reported every freshly written note as:
///
///     D  events/<uuid>.json
///
/// a *staged deletion*. No data was ever lost by this (all reads go through
/// git objects), but the output reads as catastrophic loss, and the reflex to
/// clean it up (`git add -A`, `git commit -a`, `git stash`) commits a REAL
/// deletion of every event from HEAD. Two such false alarms were reported in
/// the field before it was root-caused.
///
/// This test fails on the pre-fix code at the `is_file()` assertion.
#[test]
fn append_leaves_worktree_and_index_clean() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("brain");
    let brain = BrainRepo::init(&path).unwrap();

    let event_ref = brain
        .append_event(sample_draft("test::worktree::clean", "materialized"))
        .unwrap();

    // 1. The event is actually on disk, not just in the object database.
    let on_disk = path.join("events").join(format!("{}.json", event_ref.event_id));
    assert!(
        on_disk.is_file(),
        "event was committed but never materialized at {}",
        on_disk.display()
    );

    // 2. It is the real payload, not a placeholder.
    let raw = std::fs::read_to_string(&on_disk).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        parsed["event_id"].as_str().unwrap(),
        event_ref.event_id.to_string(),
        "on-disk blob is not this event"
    );

    // 3. HEAD, index, and worktree all agree: `git status` is empty.
    //    This is the assertion that would have caught the original defect as
    //    a staged deletion (INDEX_DELETED) rather than a missing file.
    let repo = git2::Repository::open(&path).unwrap();
    let statuses = repo.statuses(None).unwrap();
    let dirty: Vec<String> = statuses
        .iter()
        .map(|s| format!("{:?} {}", s.status(), s.path().unwrap_or("?")))
        .collect();
    assert!(
        dirty.is_empty(),
        "expected a clean working tree after append, got: {dirty:?}"
    );
}

/// The same invariant must hold across repeated appends, not just the first.
/// A per-append index write that clobbered earlier entries would pass the
/// single-append test above and fail here.
#[test]
fn repeated_appends_keep_worktree_clean() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("brain");
    let brain = BrainRepo::init(&path).unwrap();

    for i in 0..5 {
        brain
            .append_event(sample_draft(&format!("test::repeat::{i}"), "n"))
            .unwrap();
    }

    let repo = git2::Repository::open(&path).unwrap();
    let dirty: Vec<String> = repo
        .statuses(None)
        .unwrap()
        .iter()
        .map(|s| format!("{:?} {}", s.status(), s.path().unwrap_or("?")))
        .collect();
    assert!(
        dirty.is_empty(),
        "worktree dirty after 5 appends: {dirty:?}"
    );

    let events_dir = std::fs::read_dir(path.join("events")).unwrap().count();
    assert_eq!(events_dir, 5, "all 5 events should be on disk");
}
