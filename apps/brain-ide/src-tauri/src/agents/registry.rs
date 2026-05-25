//! Agent registry. Owns one instance of each agent slug and exposes
//! them by name.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::settings::Settings;

use super::claude::{ClaudeChat, ClaudeCli};
use super::codex::{CodexChat, CodexCli};
use super::human::HumanAssignee;
use super::shell::ShellAgent;
use super::types::{AgentDescriptor, CardAgent, ChatAgent};

pub type AgentSlug = String;

#[derive(Clone)]
pub struct AgentRegistry {
    chat: Arc<RwLock<HashMap<AgentSlug, Arc<dyn ChatAgent>>>>,
    card: Arc<RwLock<HashMap<AgentSlug, Arc<dyn CardAgent>>>>,
}

impl AgentRegistry {
    /// Build the default registry: Claude API + CLI, Codex API + CLI,
    /// shell, human. Settings provide model/binary defaults.
    pub fn default_for(settings: &Settings) -> Self {
        let chat: HashMap<AgentSlug, Arc<dyn ChatAgent>> = [
            (
                "claude".to_string(),
                Arc::new(ClaudeChat::new(settings.claude.clone())) as Arc<dyn ChatAgent>,
            ),
            (
                "codex".to_string(),
                Arc::new(CodexChat::new(settings.codex.clone())) as Arc<dyn ChatAgent>,
            ),
        ]
        .into_iter()
        .collect();

        let card: HashMap<AgentSlug, Arc<dyn CardAgent>> = [
            (
                "claude".to_string(),
                Arc::new(ClaudeCli::new(settings.claude.clone())) as Arc<dyn CardAgent>,
            ),
            (
                "codex".to_string(),
                Arc::new(CodexCli::new(settings.codex.clone())) as Arc<dyn CardAgent>,
            ),
            (
                "shell".to_string(),
                Arc::new(ShellAgent::new(settings.shell.clone())) as Arc<dyn CardAgent>,
            ),
            ("human".to_string(), Arc::new(HumanAssignee) as Arc<dyn CardAgent>),
        ]
        .into_iter()
        .collect();

        Self {
            chat: Arc::new(RwLock::new(chat)),
            card: Arc::new(RwLock::new(card)),
        }
    }

    pub fn chat(&self, slug: &str) -> Option<Arc<dyn ChatAgent>> {
        self.chat.read().get(slug).cloned()
    }

    pub fn card(&self, slug: &str) -> Option<Arc<dyn CardAgent>> {
        self.card.read().get(slug).cloned()
    }

    /// Refresh agents after the user changes settings or API keys.
    pub fn rebuild(&self, settings: &Settings) {
        let new = Self::default_for(settings);
        *self.chat.write() = std::mem::take(&mut new.chat.write());
        *self.card.write() = std::mem::take(&mut new.card.write());
    }

    pub fn descriptors(&self) -> Vec<AgentDescriptor> {
        let mut seen: HashMap<String, AgentDescriptor> = HashMap::new();
        for (slug, agent) in self.chat.read().iter() {
            seen.insert(slug.clone(), agent.descriptor());
        }
        for (slug, agent) in self.card.read().iter() {
            // Merge: if both chat + card exist, OR the capability flags.
            let mut d = agent.descriptor();
            if let Some(existing) = seen.get(slug) {
                d.supports_chat = d.supports_chat || existing.supports_chat;
                d.supports_card_execution =
                    d.supports_card_execution || existing.supports_card_execution;
                d.ready = d.ready || existing.ready;
                d.available_models = merge_unique(&existing.available_models, &d.available_models);
            }
            seen.insert(slug.clone(), d);
        }
        let mut out: Vec<AgentDescriptor> = seen.into_values().collect();
        out.sort_by(|a, b| a.slug.cmp(&b.slug));
        out
    }
}

fn merge_unique(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = a.to_vec();
    for s in b {
        if !out.iter().any(|x| x == s) {
            out.push(s.clone());
        }
    }
    out
}
