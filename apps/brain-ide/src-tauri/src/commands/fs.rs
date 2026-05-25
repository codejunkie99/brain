//! Project file-tree + read/write commands. Respects `.gitignore` via
//! the `ignore` crate so the tree doesn't drown in `node_modules`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct TreeNode {
    pub name: String,
    pub path: PathBuf,
    pub kind: TreeKind,
    pub children: Option<Vec<TreeNode>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeKind {
    Dir,
    File,
}

#[derive(Debug, Deserialize)]
pub struct ListTreeInput {
    pub root: Option<String>,
    /// Max depth to recurse. 0 means just the root.
    pub depth: Option<usize>,
    /// Max nodes to return overall (cheap cap on huge repos).
    pub max_nodes: Option<usize>,
}

#[tauri::command]
pub fn list_tree(
    state: State<'_, AppState>,
    input: ListTreeInput,
) -> Result<TreeNode, String> {
    let root_str = input
        .root
        .or_else(|| state.orchestrator.project_root())
        .ok_or_else(|| "no project root".to_string())?;
    let root = PathBuf::from(&root_str);
    let depth = input.depth.unwrap_or(4);
    let max_nodes = input.max_nodes.unwrap_or(2000);

    let walker = ignore::WalkBuilder::new(&root)
        .max_depth(Some(depth + 1))
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    let mut all: Vec<(PathBuf, bool)> = Vec::new();
    for (i, entry) in walker.enumerate() {
        if i >= max_nodes {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let is_dir = entry
            .file_type()
            .map(|f| f.is_dir())
            .unwrap_or(false);
        all.push((entry.into_path(), is_dir));
    }

    Ok(build_tree(&root, &all))
}

fn build_tree(root: &Path, entries: &[(PathBuf, bool)]) -> TreeNode {
    // Index children by parent path for O(n) construction.
    use std::collections::HashMap;
    let mut by_parent: HashMap<PathBuf, Vec<&(PathBuf, bool)>> = HashMap::new();
    for ent in entries {
        if let Some(parent) = ent.0.parent() {
            by_parent
                .entry(parent.to_path_buf())
                .or_default()
                .push(ent);
        }
    }
    fn node_from(
        path: &Path,
        is_dir: bool,
        by_parent: &std::collections::HashMap<PathBuf, Vec<&(PathBuf, bool)>>,
    ) -> TreeNode {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        if is_dir {
            let mut children: Vec<TreeNode> = by_parent
                .get(path)
                .map(|v| {
                    v.iter()
                        .map(|(p, d)| node_from(p, *d, by_parent))
                        .collect()
                })
                .unwrap_or_default();
            children.sort_by(|a, b| match (&a.kind, &b.kind) {
                (TreeKind::Dir, TreeKind::File) => std::cmp::Ordering::Less,
                (TreeKind::File, TreeKind::Dir) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            });
            TreeNode {
                name,
                path: path.to_path_buf(),
                kind: TreeKind::Dir,
                children: Some(children),
            }
        } else {
            TreeNode {
                name,
                path: path.to_path_buf(),
                kind: TreeKind::File,
                children: None,
            }
        }
    }
    node_from(root, true, &by_parent)
}

#[derive(Debug, Deserialize)]
pub struct ReadFileInput {
    pub path: String,
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ReadFileOutput {
    pub content: String,
    pub truncated: bool,
}

#[tauri::command]
pub fn read_file(input: ReadFileInput) -> Result<ReadFileOutput, String> {
    let max = input.max_bytes.unwrap_or(2 * 1024 * 1024);
    let raw = std::fs::read(&input.path).map_err(|e| e.to_string())?;
    let truncated = raw.len() > max;
    let slice = if truncated { &raw[..max] } else { &raw };
    let content = String::from_utf8_lossy(slice).to_string();
    Ok(ReadFileOutput { content, truncated })
}

#[derive(Debug, Deserialize)]
pub struct WriteFileInput {
    pub path: String,
    pub content: String,
}

#[tauri::command]
pub fn write_file(input: WriteFileInput) -> Result<(), String> {
    if let Some(parent) = Path::new(&input.path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&input.path, input.content.as_bytes()).map_err(|e| e.to_string())
}
