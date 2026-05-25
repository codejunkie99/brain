//! Spec parser: free-form spec text -> structured `SpecBreakdown`.
//!
//! Two backends:
//!
//!   - `LlmSpecParser`: ships the spec to an LLM with a strict JSON
//!     schema and parses the structured output. This is the primary
//!     path the IDE uses. The crate exposes the trait + a stub; the
//!     IDE wires up the real Claude/OpenAI client.
//!
//!   - `HeuristicSpecParser`: bullet-list extractor. Splits on numbered
//!     items / blank-line paragraphs. Used when no API key is configured
//!     and for offline/test mode. Better than nothing; the user will
//!     usually click "re-plan with model" once an agent is wired.
//!
//! Output is a list of cards plus a list of `(parent_title, child_title)`
//! dependency hints. The caller resolves titles to CardIds when grafting
//! the breakdown into a `FeatureGraph`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::card::{Card, CardKind};

#[derive(Debug, Error)]
pub enum SpecParserError {
    #[error("backend returned no cards")]
    Empty,
    #[error("backend response was not valid JSON: {0}")]
    BadJson(#[from] serde_json::Error),
    #[error("backend error: {0}")]
    Backend(String),
}

/// Result of parsing a spec.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpecBreakdown {
    pub cards: Vec<Card>,
    /// Dependencies expressed as `(from_title, to_title)` — `from` must
    /// complete before `to` starts. Caller resolves titles to CardIds.
    pub deps: Vec<(String, String)>,
    /// Short rationale the model produced; surfaced to the user.
    pub rationale: Option<String>,
}

#[async_trait]
pub trait SpecParser: Send + Sync {
    async fn parse(&self, spec: &str) -> Result<SpecBreakdown, SpecParserError>;
}

/// Heuristic, dependency-free spec parser. Splits the spec into cards
/// by looking for numbered list items first, then blank-line paragraphs.
///
/// Limitations: it can't infer dependencies, so the resulting graph is
/// a flat fan-out from a single root. The UI exposes "re-plan with
/// Claude" / "re-plan with Codex" to upgrade this.
pub struct HeuristicSpecParser;

#[async_trait]
impl SpecParser for HeuristicSpecParser {
    async fn parse(&self, spec: &str) -> Result<SpecBreakdown, SpecParserError> {
        let chunks = chunk_spec(spec);
        if chunks.is_empty() {
            return Err(SpecParserError::Empty);
        }
        let cards = chunks
            .into_iter()
            .map(|chunk| {
                let (title, body) = split_title_body(&chunk);
                let kind = infer_kind(&title, &body);
                Card::new(title, body, kind)
            })
            .collect();
        Ok(SpecBreakdown {
            cards,
            deps: Vec::new(),
            rationale: Some("Parsed heuristically; assign an agent and re-plan to infer dependencies.".into()),
        })
    }
}

fn chunk_spec(spec: &str) -> Vec<String> {
    // First pass: numbered list (`1.`, `1)`, `- `, `* `).
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_list = false;

    for line in spec.lines() {
        let trimmed = line.trim_start();
        let is_item = is_list_item(trimmed);
        if is_item {
            if !current.trim().is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            current.push_str(strip_bullet(trimmed));
            current.push('\n');
            in_list = true;
        } else if in_list && line.trim().is_empty() {
            // blank line continues the current item until the next bullet
            current.push('\n');
        } else if in_list {
            current.push_str(line);
            current.push('\n');
        } else {
            // Not yet in a list; collect paragraphs as fallback.
            if line.trim().is_empty() {
                if !current.trim().is_empty() {
                    chunks.push(std::mem::take(&mut current));
                }
            } else {
                current.push_str(line);
                current.push('\n');
            }
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn is_list_item(s: &str) -> bool {
    if s.starts_with("- ") || s.starts_with("* ") {
        return true;
    }
    let mut chars = s.chars();
    let first = chars.next();
    let second = chars.next();
    matches!((first, second), (Some(c), Some(d)) if c.is_ascii_digit() && (d == '.' || d == ')'))
}

fn strip_bullet(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")) {
        return rest;
    }
    // Numbered: drop the "<digits>[.)] "
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
        let after = i + 1;
        if after < bytes.len() && bytes[after] == b' ' {
            return &s[after + 1..];
        }
        return &s[after..];
    }
    s
}

fn split_title_body(chunk: &str) -> (String, String) {
    let trimmed = chunk.trim();
    let (first, rest) = match trimmed.split_once('\n') {
        Some((a, b)) => (a, b),
        None => (trimmed, ""),
    };
    let title = first.trim().to_string();
    let body = rest.trim().to_string();
    // Title shouldn't exceed ~80 chars; if the chunk is a single long
    // sentence, take the first 80 chars as the title and put the
    // whole thing in the body.
    if title.chars().count() > 80 {
        let mut truncated: String = title.chars().take(77).collect();
        truncated.push('…');
        (truncated, trimmed.to_string())
    } else {
        (title, body)
    }
}

fn infer_kind(title: &str, body: &str) -> CardKind {
    let hay = format!("{} {}", title.to_lowercase(), body.to_lowercase());
    if hay.contains("bug") || hay.contains("fix") || hay.contains("regression") {
        CardKind::BugFix
    } else if hay.contains("refactor") || hay.contains("rewrite") || hay.contains("clean up") {
        CardKind::Refactor
    } else if hay.contains("research") || hay.contains("investigate") || hay.contains("explore") {
        CardKind::Research
    } else if hay.contains("test") || hay.contains("spec") || hay.contains("coverage") {
        CardKind::Test
    } else if hay.contains("docs") || hay.contains("documentation") || hay.contains("readme") {
        CardKind::Docs
    } else if hay.contains("setup") || hay.contains("scaffold") || hay.contains("install") {
        CardKind::Setup
    } else {
        CardKind::Feature
    }
}

/// JSON envelope the LLM is asked to emit. Keep this stable — it's the
/// contract with the model.
///
/// Example:
/// ```json
/// {
///   "rationale": "Split into data, API, and UI layers.",
///   "cards": [
///     {"title": "Add migration", "summary": "...", "kind": "setup"},
///     {"title": "Wire API endpoint", "summary": "...", "kind": "feature"}
///   ],
///   "deps": [["Add migration", "Wire API endpoint"]]
/// }
/// ```
#[derive(Debug, Deserialize)]
struct LlmEnvelope {
    rationale: Option<String>,
    cards: Vec<LlmCard>,
    #[serde(default)]
    deps: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
struct LlmCard {
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    suggested_agent: Option<String>,
}

/// Parse the strict JSON envelope produced by an LLM into a
/// `SpecBreakdown`. This is the function the IDE calls after the model
/// streams its response — the API client lives in the IDE crate, but
/// the schema lives here so the prompt template and the parser stay in
/// the same file.
pub fn parse_llm_envelope(json: &str) -> Result<SpecBreakdown, SpecParserError> {
    let env: LlmEnvelope = serde_json::from_str(json)?;
    if env.cards.is_empty() {
        return Err(SpecParserError::Empty);
    }
    let cards = env
        .cards
        .into_iter()
        .map(|c| {
            let kind = c
                .kind
                .as_deref()
                .map(parse_kind)
                .unwrap_or(CardKind::Feature);
            let mut card = Card::new(c.title, c.summary, kind);
            card.assigned_agent = c.suggested_agent;
            card
        })
        .collect();
    Ok(SpecBreakdown {
        cards,
        deps: env.deps.into_iter().map(|[a, b]| (a, b)).collect(),
        rationale: env.rationale,
    })
}

fn parse_kind(s: &str) -> CardKind {
    match s.to_ascii_lowercase().as_str() {
        "feature" => CardKind::Feature,
        "refactor" => CardKind::Refactor,
        "bug" | "bugfix" | "bug_fix" | "bug-fix" => CardKind::BugFix,
        "research" => CardKind::Research,
        "test" | "tests" => CardKind::Test,
        "docs" | "documentation" => CardKind::Docs,
        "setup" => CardKind::Setup,
        _ => CardKind::Feature,
    }
}

/// Prompt template the IDE sends to the model. Returned as a string
/// rather than embedded so the IDE can edit/tune it in one place.
pub fn spec_breakdown_prompt(spec: &str) -> String {
    format!(
        "You are a senior staff engineer breaking a feature spec into a directed acyclic graph of work items for a multi-agent coding team.\n\
\n\
Spec:\n---\n{spec}\n---\n\
\n\
Return STRICT JSON matching this schema and nothing else:\n\
{{\n  \"rationale\": \"one short paragraph on how you split this\",\n  \"cards\": [{{\"title\": \"...\", \"summary\": \"...\", \"kind\": \"feature|refactor|bug_fix|research|test|docs|setup\", \"suggested_agent\": \"claude|codex|shell|null\"}}],\n  \"deps\": [[\"title that must finish first\", \"title that depends on it\"], ...]\n}}\n\
\n\
Rules:\n\
- 3-10 cards. Each card is a single discrete unit a coding agent can finish in under an hour.\n\
- Titles are unique. Reference cards in `deps` by exact title.\n\
- `deps` must form a DAG (no cycles).\n\
- Choose `suggested_agent` only when a card clearly favors one tool (e.g. shell for ops, codex for stylistic refactors, claude for design/architecture). Leave null if unsure.\n\
- Output ONLY the JSON object. No prose before or after.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn heuristic_splits_numbered_list() {
        let parser = HeuristicSpecParser;
        let spec = "1. Add login\n2. Build dashboard\n3. Wire analytics";
        let breakdown = parser.parse(spec).await.unwrap();
        assert_eq!(breakdown.cards.len(), 3);
        assert_eq!(breakdown.cards[0].title, "Add login");
    }

    #[test]
    fn envelope_parses() {
        let json = r#"{
            "rationale": "split into data, api, ui",
            "cards": [
                {"title": "Migration", "summary": "Add users table", "kind": "setup"},
                {"title": "API", "summary": "POST /signup", "kind": "feature", "suggested_agent": "claude"},
                {"title": "UI", "summary": "Signup form", "kind": "feature"}
            ],
            "deps": [["Migration", "API"], ["API", "UI"]]
        }"#;
        let b = parse_llm_envelope(json).unwrap();
        assert_eq!(b.cards.len(), 3);
        assert_eq!(b.deps.len(), 2);
        assert_eq!(b.cards[1].assigned_agent.as_deref(), Some("claude"));
    }

    #[test]
    fn envelope_with_empty_cards_is_error() {
        let json = r#"{"rationale": null, "cards": [], "deps": []}"#;
        assert!(matches!(
            parse_llm_envelope(json).unwrap_err(),
            SpecParserError::Empty
        ));
    }
}
