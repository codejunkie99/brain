//! Git plumbing the IDE needs: status, diff, branch info, commit log,
//! commit + checkout. Backed by libgit2 so we don't shell out to `git`
//! and don't depend on a user-installed binary.
//!
//! All paths are absolute. The frontend passes the project root every
//! call so we never silently resolve "the wrong repo" because we
//! cached one.

use std::path::{Path, PathBuf};

use git2::{
    DiffFormat, DiffLineType, DiffOptions, IndexAddOption, Repository, Sort, StatusOptions,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("not a git repository: {0}")]
    NotARepo(String),
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoStatus {
    pub root: PathBuf,
    pub head: Option<HeadInfo>,
    pub branches: Vec<BranchInfo>,
    pub files: Vec<StatusEntry>,
    pub remotes: Vec<RemoteInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeadInfo {
    pub branch: Option<String>,
    pub detached: bool,
    pub commit: String,
    pub ahead: usize,
    pub behind: usize,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteInfo {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusEntry {
    pub path: String,
    pub stage: StageKind,
    pub work: WorkKind,
    pub conflict: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    None,
    Added,
    Modified,
    Deleted,
    Renamed,
    Typechange,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    None,
    Modified,
    Deleted,
    Typechange,
    Renamed,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    pub kind: char,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub binary: bool,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
    pub author: String,
    pub email: String,
    pub time: i64,
}

fn open_repo(root: &Path) -> Result<Repository, GitError> {
    Repository::discover(root).map_err(|_| GitError::NotARepo(root.display().to_string()))
}

pub fn status(root: &Path) -> Result<RepoStatus, GitError> {
    let repo = open_repo(root)?;
    let head = head_info(&repo).ok();
    let branches = branch_list(&repo)?;
    let files = file_status(&repo)?;
    let remotes = remote_list(&repo)?;

    let workdir = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| repo.path().to_path_buf());

    Ok(RepoStatus {
        root: workdir,
        head,
        branches,
        files,
        remotes,
    })
}

fn head_info(repo: &Repository) -> Result<HeadInfo, GitError> {
    let head = repo.head()?;
    let detached = repo.head_detached().unwrap_or(false);
    let branch = if detached {
        None
    } else {
        head.shorthand().map(|s| s.to_string())
    };
    let commit = head.peel_to_commit()?.id().to_string();

    let mut ahead = 0;
    let mut behind = 0;
    let mut upstream: Option<String> = None;

    if let Some(name) = head.shorthand() {
        if let Ok(local) = repo.find_branch(name, git2::BranchType::Local) {
            if let Ok(up) = local.upstream() {
                if let Some(up_name) = up.name().ok().flatten() {
                    upstream = Some(up_name.to_string());
                }
                let local_oid = local.get().target();
                let up_oid = up.get().target();
                if let (Some(a), Some(b)) = (local_oid, up_oid) {
                    if let Ok((ah, be)) = repo.graph_ahead_behind(a, b) {
                        ahead = ah;
                        behind = be;
                    }
                }
            }
        }
    }

    Ok(HeadInfo {
        branch,
        detached,
        commit,
        ahead,
        behind,
        upstream,
    })
}

fn branch_list(repo: &Repository) -> Result<Vec<BranchInfo>, GitError> {
    let head_shorthand = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));
    let mut out = Vec::new();
    for branch in repo.branches(None)? {
        let (b, kind) = branch?;
        let name = b.name()?.unwrap_or("").to_string();
        let is_remote = kind == git2::BranchType::Remote;
        let is_head = !is_remote && Some(name.clone()) == head_shorthand;
        out.push(BranchInfo {
            name,
            is_head,
            is_remote,
        });
    }
    out.sort_by(|a, b| (a.is_remote, &a.name).cmp(&(b.is_remote, &b.name)));
    Ok(out)
}

fn remote_list(repo: &Repository) -> Result<Vec<RemoteInfo>, GitError> {
    let mut out = Vec::new();
    for name in repo.remotes()?.iter().flatten() {
        let url = repo.find_remote(name).ok().and_then(|r| r.url().map(|s| s.to_string()));
        out.push(RemoteInfo {
            name: name.to_string(),
            url,
        });
    }
    Ok(out)
}

fn file_status(repo: &Repository) -> Result<Vec<StatusEntry>, GitError> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut out = Vec::new();
    for entry in statuses.iter() {
        let bits = entry.status();
        let path = entry.path().unwrap_or("").to_string();
        let stage = stage_from(bits);
        let work = work_from(bits);
        if stage == StageKind::None && work == WorkKind::None {
            continue;
        }
        out.push(StatusEntry {
            path,
            stage,
            work,
            conflict: bits.contains(git2::Status::CONFLICTED),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn stage_from(s: git2::Status) -> StageKind {
    if s.contains(git2::Status::INDEX_NEW) {
        StageKind::Added
    } else if s.contains(git2::Status::INDEX_MODIFIED) {
        StageKind::Modified
    } else if s.contains(git2::Status::INDEX_DELETED) {
        StageKind::Deleted
    } else if s.contains(git2::Status::INDEX_RENAMED) {
        StageKind::Renamed
    } else if s.contains(git2::Status::INDEX_TYPECHANGE) {
        StageKind::Typechange
    } else {
        StageKind::None
    }
}

fn work_from(s: git2::Status) -> WorkKind {
    if s.contains(git2::Status::WT_NEW) {
        WorkKind::Untracked
    } else if s.contains(git2::Status::WT_MODIFIED) {
        WorkKind::Modified
    } else if s.contains(git2::Status::WT_DELETED) {
        WorkKind::Deleted
    } else if s.contains(git2::Status::WT_RENAMED) {
        WorkKind::Renamed
    } else if s.contains(git2::Status::WT_TYPECHANGE) {
        WorkKind::Typechange
    } else if s.contains(git2::Status::IGNORED) {
        WorkKind::Ignored
    } else {
        WorkKind::None
    }
}

pub fn diff(
    root: &Path,
    path: &str,
    staged: bool,
) -> Result<FileDiff, GitError> {
    let repo = open_repo(root)?;
    let mut opts = DiffOptions::new();
    opts.pathspec(path)
        .context_lines(3)
        .interhunk_lines(0)
        .show_untracked_content(true)
        .recurse_untracked_dirs(true);

    let diff = if staged {
        let head_tree = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_tree().ok());
        repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))?
    } else {
        repo.diff_index_to_workdir(None, Some(&mut opts))?
    };

    let mut binary = false;
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_hunk_idx: Option<usize> = None;

    diff.print(DiffFormat::Patch, |_delta, hunk, line| {
        if hunk.is_some() && current_hunk_idx.is_none() {
            // Initial header.
        }
        if let Some(h) = hunk {
            // A new hunk opens.
            if current_hunk_idx.map_or(true, |i| {
                hunks[i].old_start != h.old_start() || hunks[i].new_start != h.new_start()
            }) {
                let header = std::str::from_utf8(h.header()).unwrap_or("").to_string();
                hunks.push(DiffHunk {
                    old_start: h.old_start(),
                    old_lines: h.old_lines(),
                    new_start: h.new_start(),
                    new_lines: h.new_lines(),
                    header,
                    lines: Vec::new(),
                });
                current_hunk_idx = Some(hunks.len() - 1);
            }
        }
        if let Some(idx) = current_hunk_idx {
            let kind = match line.origin_value() {
                DiffLineType::Addition => '+',
                DiffLineType::Deletion => '-',
                DiffLineType::Context => ' ',
                DiffLineType::Binary => {
                    binary = true;
                    'B'
                }
                _ => ' ',
            };
            let content = std::str::from_utf8(line.content())
                .unwrap_or("")
                .trim_end_matches('\n')
                .to_string();
            hunks[idx].lines.push(DiffLine {
                kind,
                content,
                old_lineno: line.old_lineno(),
                new_lineno: line.new_lineno(),
            });
        }
        true
    })?;

    Ok(FileDiff {
        path: path.to_string(),
        binary,
        hunks,
    })
}

pub fn stage(root: &Path, paths: &[String]) -> Result<(), GitError> {
    let repo = open_repo(root)?;
    let mut index = repo.index()?;
    let pathspecs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    index.add_all(pathspecs.iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    Ok(())
}

pub fn unstage(root: &Path, paths: &[String]) -> Result<(), GitError> {
    let repo = open_repo(root)?;
    let head = repo.head().ok().and_then(|h| h.peel(git2::ObjectType::Commit).ok());
    if let Some(head) = head {
        repo.reset_default(Some(&head), paths)?;
    } else {
        // No HEAD yet (initial commit) — strip from the index.
        let mut index = repo.index()?;
        for p in paths {
            let _ = index.remove_path(Path::new(p));
        }
        index.write()?;
    }
    Ok(())
}

pub fn commit(root: &Path, message: &str) -> Result<String, GitError> {
    if message.trim().is_empty() {
        return Err(GitError::Other("commit message is empty".into()));
    }
    let repo = open_repo(root)?;
    let sig = repo.signature()?;
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

    let parents: Vec<&git2::Commit> = head_commit.as_ref().map(|c| vec![c]).unwrap_or_default();

    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
    Ok(oid.to_string())
}

pub fn log(root: &Path, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
    let repo = open_repo(root)?;
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME)?;
    walk.push_head()?;
    let mut out = Vec::new();
    for oid in walk.take(limit) {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        out.push(CommitInfo {
            oid: oid.to_string(),
            short_oid: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("").to_string(),
            author: commit.author().name().unwrap_or("").to_string(),
            email: commit.author().email().unwrap_or("").to_string(),
            time: commit.time().seconds(),
        });
    }
    Ok(out)
}

pub fn checkout(root: &Path, branch: &str) -> Result<(), GitError> {
    let repo = open_repo(root)?;
    // Try local branch first, then resolve a remote one by name.
    let refname = if repo.find_branch(branch, git2::BranchType::Local).is_ok() {
        format!("refs/heads/{branch}")
    } else if let Ok(remote_branch) = repo.find_branch(branch, git2::BranchType::Remote) {
        // Materialize a local tracking branch.
        let commit = remote_branch.get().peel_to_commit()?;
        let new_local = repo.branch(branch, &commit, false)?;
        let _ = new_local;
        format!("refs/heads/{branch}")
    } else {
        return Err(GitError::Other(format!("branch '{branch}' not found")));
    };
    let obj = repo.revparse_single(&refname)?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head(&refname)?;
    Ok(())
}
