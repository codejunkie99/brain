//! API key storage. Backed by the macOS Keychain via the `keyring` crate.
//!
//! Two scopes:
//!   - "anthropic" for Claude API access
//!   - "openai" for Codex / GPT API access
//!
//! Frontend never sees the key value — it only sets it (write-only) and
//! reads back a boolean "is configured" flag.

use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.brain.ide";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Anthropic,
    Openai,
    Custom,
}

impl CredentialKind {
    fn account(self) -> &'static str {
        match self {
            CredentialKind::Anthropic => "anthropic",
            CredentialKind::Openai => "openai",
            CredentialKind::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialStatus {
    pub kind: CredentialKind,
    pub is_set: bool,
}

pub struct AuthStore;

impl AuthStore {
    pub fn set(kind: CredentialKind, secret: &str) -> anyhow::Result<()> {
        let entry = keyring::Entry::new(SERVICE, kind.account())?;
        entry.set_password(secret)?;
        Ok(())
    }

    pub fn clear(kind: CredentialKind) -> anyhow::Result<()> {
        let entry = keyring::Entry::new(SERVICE, kind.account())?;
        // delete_credential errors if it doesn't exist; that's fine
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Read the secret. Only the agent runners call this — never the
    /// frontend. Returns None if no credential is set.
    pub fn get(kind: CredentialKind) -> anyhow::Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, kind.account())?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn status(kind: CredentialKind) -> CredentialStatus {
        CredentialStatus {
            kind,
            is_set: Self::get(kind).ok().flatten().is_some(),
        }
    }
}
