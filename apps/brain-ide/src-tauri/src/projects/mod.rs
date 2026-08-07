//! IDE projects.
//!
//! A "project" is a (name, path) pair pointing at a directory on disk
//! plus the path to its brain memory dir. The default brain dir is
//! `<project>/.brain`, but users can point multiple projects at one
//! shared brain too.
//!
//! Persisted to `projects.json` alongside `settings.json`. Each open
//! has at most one active project.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub root: PathBuf,
    /// Where this project's brain memory lives. Defaults to root/.brain.
    pub brain_dir: PathBuf,
    pub created_at: DateTime<Utc>,
    pub last_opened: DateTime<Utc>,
}

impl Project {
    pub fn new(name: impl Into<String>, root: PathBuf, brain_dir: Option<PathBuf>) -> Self {
        let brain_dir = brain_dir.unwrap_or_else(|| root.join(".brain"));
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            name: name.into(),
            root,
            brain_dir,
            created_at: now,
            last_opened: now,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectsFile {
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default)]
    active: Option<Uuid>,
}

#[derive(Clone)]
pub struct ProjectStore {
    inner: Arc<RwLock<ProjectsFile>>,
    file: PathBuf,
}

impl ProjectStore {
    pub fn open_or_default(file: PathBuf) -> Self {
        let initial: ProjectsFile = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            inner: Arc::new(RwLock::new(initial)),
            file,
        }
    }

    pub fn list(&self) -> Vec<Project> {
        self.inner.read().projects.clone()
    }

    pub fn active(&self) -> Option<Project> {
        let guard = self.inner.read();
        let id = guard.active?;
        guard.projects.iter().find(|p| p.id == id).cloned()
    }

    pub fn add(&self, project: Project) -> anyhow::Result<Project> {
        let id = project.id;
        {
            let mut g = self.inner.write();
            // Refuse to add a duplicate by root path.
            if g.projects.iter().any(|p| p.root == project.root) {
                anyhow::bail!("project at {} already exists", project.root.display());
            }
            g.projects.push(project.clone());
            if g.active.is_none() {
                g.active = Some(id);
            }
        }
        self.persist()?;
        Ok(project)
    }

    pub fn remove(&self, id: Uuid) -> anyhow::Result<()> {
        {
            let mut g = self.inner.write();
            g.projects.retain(|p| p.id != id);
            if g.active == Some(id) {
                g.active = g.projects.first().map(|p| p.id);
            }
        }
        self.persist()
    }

    pub fn set_active(&self, id: Uuid) -> anyhow::Result<Project> {
        let project = {
            let mut g = self.inner.write();
            let project = g
                .projects
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| anyhow::anyhow!("project {id} not found"))?;
            project.last_opened = Utc::now();
            let p = project.clone();
            g.active = Some(id);
            p
        };
        self.persist()?;
        Ok(project)
    }

    pub fn resolve_path(&self, path: &Path) -> Option<Project> {
        self.inner
            .read()
            .projects
            .iter()
            .find(|p| p.root == path)
            .cloned()
    }

    fn persist(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.file.with_extension("json.tmp");
        let snapshot = self.inner.read();
        std::fs::write(&tmp, serde_json::to_vec_pretty(&*snapshot)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}
