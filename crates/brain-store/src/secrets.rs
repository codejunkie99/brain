//! v0.2 secret prefilter. Fast regex scan over the *fully serialized*
//! `Event` JSON. If any pattern matches, the write is rejected BEFORE
//! commit, so the secret never enters the git history.
//!
//! Scanning the serialized JSON (rather than a hand-picked list of
//! payload fields) closes the main bypass: every future string field on
//! every payload variant is covered automatically. The cost is roughly
//! one RegexSet pass over ~1 KB of JSON per append, which is nothing.
//!
//! This is still deliberately conservative and pattern-based. It will
//! miss novel tokens, custom shapes, and high-entropy strings that
//! don't match any registered regex. A deeper scan (gitleaks) belongs
//! on the `export --safe` path in a follow-up.

use brain_types::EventPayload;
use once_cell::sync::Lazy;
use regex::RegexSet;
use unicode_normalization::UnicodeNormalization;

/// Named secret patterns. Names are returned to the caller so they see
/// WHICH kind of secret was detected without the secret itself leaking.
///
/// Order matters only for the return value: the first pattern in the
/// list that matches wins. Ranked roughly by specificity so the most
/// informative name surfaces.
const RAW_PATTERNS: &[(&str, &str)] = &[
    // Anthropic FIRST. The openai-key pattern below is a superset of
    // `sk-ant-...` and would otherwise win in the RegexSet (lowest index
    // wins). Rust's `regex` crate has no lookahead, so the anti-overlap
    // has to come from ordering.
    ("anthropic-key", r"\bsk-ant-[A-Za-z0-9_-]{20,}\b"),
    // OpenAI — covers both legacy `sk-...` and modern project keys `sk-proj-...`.
    (
        "openai-key",
        r"\bsk-(?:proj-)?[A-Za-z0-9][A-Za-z0-9_-]{20,}\b",
    ),
    // GitHub tokens: classic PATs (ghp_), OAuth (gho_), user-to-server
    // (ghu_), server-to-server (ghs_), refresh (ghr_), and fine-grained PATs.
    (
        "github-token",
        r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{80,})\b",
    ),
    // AWS access key IDs (long-lived AKIA, short-lived ASIA).
    ("aws-access-key-id", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
    // AWS secret access keys: 40 chars of base64-ish. False-positives
    // if unanchored, so require an adjacent label. The label match is
    // case-insensitive and covers snake_case, camelCase, PascalCase,
    // and dashed variants ("secret_access_key", "secretAccessKey",
    // "SecretAccessKey", "secret-access-key", optional "aws" prefix).
    // Codex C2 round 7.
    (
        "aws-secret-access-key",
        r"(?i)\b(?:aws[_-]?)?secret[_-]?access[_-]?key\b.{0,3}[=:].{0,3}[A-Za-z0-9/+=]{40}\b",
    ),
    // Database connection URIs with embedded credentials. Matches when
    // the password component is non-empty. Codex F1 round 5: "Prefilter
    // misses DB URIs with embedded passwords."
    (
        "db-uri-credentials",
        r"\b(?:postgres|postgresql|mysql|mongodb|mongodb\+srv|redis|amqp)://[^:\s/@]+:[^@\s]{4,}@\S+",
    ),
    ("slack-token", r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b"),
    ("stripe-live-key", r"\bsk_live_[A-Za-z0-9]{24,}\b"),
    // Stripe test keys also have value (staging-leak risk). F1 round 5.
    ("stripe-test-key", r"\bsk_test_[A-Za-z0-9]{24,}\b"),
    // Google API keys. F1 round 5.
    ("google-api-key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
    // GCP service-account JSON blobs. The `"type":"service_account"`
    // marker alone is just a JSON type label and appears in plenty of
    // documentation — require the PRIVATE_KEY field with a real PEM
    // header in the same JSON so we only match on actual credential
    // material. Pattern matches when `type` appears before `private_key`
    // (the conventional ordering in real service-account files) with up
    // to 1 KiB of intervening fields.
    //
    // Backstop: when the same blob gets pasted into a Rust `String`
    // field (e.g. `Observe.content`) and then serialized, serde_json
    // escapes the inner `"` as `\"`, so this pattern no longer matches
    // the escaped form. The `pem-private-key` regex below still fires
    // because `-----BEGIN PRIVATE KEY-----` contains no quote chars and
    // survives JSON escaping intact. (See
    // `detects_gcp_blob_embedded_in_string_field` test.) Codex R11
    // flagged this as a potential bypass; the backstop closes it.
    (
        "gcp-service-account",
        r#"(?s)"type"\s*:\s*"service_account".{0,1024}"private_key"\s*:\s*"-----BEGIN"#,
    ),
    // HashiCorp Vault tokens. `hvs.` service, `hvb.` batch, `s.` legacy.
    ("vault-token", r"\b(?:hvs|hvb|s)\.[A-Za-z0-9._-]{20,}\b"),
    // Azure storage SAS signatures. The `?sv=...&sig=<b64>` query-string
    // shape is what gets accidentally pasted. F1 round 5.
    ("azure-sas", r"[?&]sig=[A-Za-z0-9%+/=_-]{20,}"),
    // Twilio account-SID + auth-token pair. Matches account SID directly;
    // the adjacent auth token is 32 hex and caught only when labeled,
    // otherwise would false-positive on any 32-hex string (md5 sums, etc.).
    ("twilio-account-sid", r"\bAC[0-9a-f]{32}\b"),
    // Labeled Twilio auth tokens — the env-var / config-file leak shape
    // that the account-SID pattern alone misses. Codex R11.
    (
        "twilio-auth-token",
        r"(?i)\btwilio[_-]?auth[_-]?token\b.{0,3}[=:].{0,3}[0-9a-f]{32}\b",
    ),
    (
        "jwt",
        r"\beyJ[A-Za-z0-9_=-]+\.[A-Za-z0-9_=-]+\.[A-Za-z0-9_.+/=-]*\b",
    ),
    (
        "pem-private-key",
        r"-----BEGIN (?:RSA |OPENSSH |DSA |EC )?PRIVATE KEY-----",
    ),
    // HTTP Authorization header bearer tokens (case-insensitive).
    ("bearer-token", r"(?i)\bbearer\s+[A-Za-z0-9._~+/-]{20,}\b"),
];

struct SecretPatterns {
    names: Vec<&'static str>,
    set: RegexSet,
}

static PATTERNS: Lazy<SecretPatterns> = Lazy::new(|| {
    let names: Vec<&'static str> = RAW_PATTERNS.iter().map(|(n, _)| *n).collect();
    let set =
        RegexSet::new(RAW_PATTERNS.iter().map(|(_, re)| *re)).expect("secret patterns compile");
    SecretPatterns { names, set }
});

/// Scan an arbitrary string. Returns the name of the lowest-index pattern
/// that matched, or None. The matched text itself is never returned — only
/// the pattern name, so error messages stay safe to log and print.
///
/// Defense against normalization bypasses runs in three passes:
///
///   1. Raw scan — catch the common case with no allocation.
///   2. Zero-width strip — remove ZWSP/ZWNJ/ZWJ/WJ/BOM and rescan so
///      "s\u{200B}k-ant-..." can't smuggle through.
///   3. NFKC normalize + zero-width strip — fold Unicode lookalikes
///      like fullwidth ASCII (ｓｋ－ｐｒｏｊ－...) and mathematical
///      alphanumerics (𝙰𝙺𝙸𝙰...) to their ASCII originals, then scan.
///      Codex F6 round 6: "fullwidth ASCII lookalikes survive the 5-ZW
///      strip and trivially bypass regex patterns anchored on ASCII."
pub fn detect_secret_text(raw: &str) -> Option<&'static str> {
    if let Some(idx) = PATTERNS.set.matches(raw).iter().next() {
        return Some(PATTERNS.names[idx]);
    }
    // Pass 2: zero-width strip.
    if raw.chars().any(is_zero_width) {
        let normalized: String = raw.chars().filter(|c| !is_zero_width(*c)).collect();
        if let Some(idx) = PATTERNS.set.matches(&normalized).iter().next() {
            return Some(PATTERNS.names[idx]);
        }
    }
    // Pass 3: NFKC fold (compatibility decomposition + canonical
    // composition) then zero-width strip. NFKC turns fullwidth ASCII,
    // mathematical alphanumerics, and similar lookalikes into plain
    // ASCII where the regexes anchor cleanly.
    let nfkc: String = raw.nfkc().filter(|c| !is_zero_width(*c)).collect();
    if nfkc != raw {
        if let Some(idx) = PATTERNS.set.matches(&nfkc).iter().next() {
            return Some(PATTERNS.names[idx]);
        }
    }
    None
}

/// Convenience wrapper: serialize an `EventPayload` to JSON and scan.
/// Covers every string field on every payload variant, present and
/// future, including nested `serde_json::Value` content. The primary
/// write-path caller (`append_event`) already serializes the full
/// `Event` and calls `detect_secret_text` directly; this helper is
/// kept for standalone payload checks and unit tests.
pub fn detect_secret(payload: &EventPayload) -> Option<&'static str> {
    match serde_json::to_string(payload) {
        Ok(s) => detect_secret_text(&s),
        // If serialization fails something is deeply wrong — refuse to
        // claim the payload is clean.
        Err(_) => Some("unserializable-payload"),
    }
}

fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_types::{ObservePayload, PrefPayload};

    fn obs(text: &str) -> EventPayload {
        EventPayload::Observe(ObservePayload {
            summary: text.to_string(),
            content: None,
            content_ref: None,
            source: None,
        })
    }

    #[test]
    fn detects_anthropic_key() {
        let p = obs("my key is sk-ant-abcdefghijklmnopqrstuvwxyz0123456789abcdef");
        assert_eq!(detect_secret(&p), Some("anthropic-key"));
    }

    #[test]
    fn detects_openai_legacy_key() {
        let p = obs("openai=sk-abcdefghijklmnopqrstuvwxyz");
        assert_eq!(detect_secret(&p), Some("openai-key"));
    }

    #[test]
    fn detects_openai_project_key() {
        // Modern "sk-proj-..." shape that the v0.1 regex missed.
        let p = obs("OPENAI_KEY=sk-proj-ABCDEFghijklmnopqrstuvwx0123");
        assert_eq!(detect_secret(&p), Some("openai-key"));
    }

    #[test]
    fn detects_aws_access_key() {
        let p = obs("cred AKIAIOSFODNN7EXAMPLE");
        assert_eq!(detect_secret(&p), Some("aws-access-key-id"));
    }

    #[test]
    fn detects_aws_short_term_access_key() {
        let p = obs("sts cred ASIAIOSFODNN7EXAMPLE");
        assert_eq!(detect_secret(&p), Some("aws-access-key-id"));
    }

    #[test]
    fn detects_aws_secret_access_key_when_labeled() {
        let p = obs("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        assert_eq!(detect_secret(&p), Some("aws-secret-access-key"));
    }

    #[test]
    fn detects_github_classic_pat() {
        let p = obs("token ghp_1234567890abcdefghij1234567890abcdef1234");
        assert_eq!(detect_secret(&p), Some("github-token"));
    }

    #[test]
    fn detects_github_oauth_token() {
        let p = obs("GH_TOKEN=gho_1234567890abcdefghij1234567890abcdef");
        assert_eq!(detect_secret(&p), Some("github-token"));
    }

    #[test]
    fn detects_pem_header() {
        let p = obs("here is my -----BEGIN RSA PRIVATE KEY----- stuff");
        assert_eq!(detect_secret(&p), Some("pem-private-key"));
    }

    #[test]
    fn detects_lowercase_bearer() {
        let p = obs("curl -H 'authorization: bearer abcdefghijklmnopqrstuvwxyz'");
        assert_eq!(detect_secret(&p), Some("bearer-token"));
    }

    #[test]
    fn detects_secret_in_pref_previous_value() {
        // This field was NOT in the old collect_haystacks allow-list.
        // The JSON scan must catch it.
        let p = EventPayload::Pref(PrefPayload {
            category: "auth".to_string(),
            key: "api-key".to_string(),
            value: serde_json::json!("rotated"),
            previous_value: Some(serde_json::json!(
                "sk-ant-abcdefghijklmnopqrstuvwxyz0123456789abcdef"
            )),
        });
        assert_eq!(detect_secret(&p), Some("anthropic-key"));
    }

    #[test]
    fn detects_secret_hidden_in_observe_content_ref() {
        // content_ref is normally a Blake3 hex, but a confused caller
        // can stuff anything into any String field. The old handpicked
        // allow-list skipped this field entirely.
        let p = EventPayload::Observe(ObservePayload {
            summary: "ok".to_string(),
            content: None,
            content_ref: Some("sk-ant-abcdefghijklmnopqrstuvwxyz0123456789abcdef".to_string()),
            source: None,
        });
        assert_eq!(detect_secret(&p), Some("anthropic-key"));
    }

    #[test]
    fn detects_secret_with_zero_width_obfuscation() {
        // An attacker/careless paste might smuggle a ZWSP into the prefix.
        let p = obs("key=sk-ant\u{200B}-abcdefghijklmnopqrstuvwxyz0123456789abcdef");
        assert_eq!(detect_secret(&p), Some("anthropic-key"));
    }

    #[test]
    fn ignores_normal_text() {
        let p = obs("I chose fastapi-users over authlib for PKCE ergonomics");
        assert_eq!(detect_secret(&p), None);
    }

    #[test]
    fn ignores_short_sk_prefix_that_isnt_a_key() {
        let p = obs("sk is a noun, not an api key");
        assert_eq!(detect_secret(&p), None);
    }

    #[test]
    fn raw_text_api_works_on_plain_strings() {
        assert_eq!(
            detect_secret_text("ghp_1234567890abcdefghij1234567890abcdef1234"),
            Some("github-token")
        );
        assert_eq!(detect_secret_text("just a boring sentence"), None);
    }

    #[test]
    fn detects_gcp_blob_pasted_as_json_object() {
        // Raw (unescaped) shape: user pastes the service-account JSON as
        // a Pref.value, where serde stores it as a JSON object, not a
        // string. Inside the serialized Event the `"type"` / `"private_key"`
        // fields appear unescaped. The gcp-service-account pattern should
        // match; if it doesn't, pem-private-key is the backstop.
        use brain_types::PrefPayload;
        let p = EventPayload::Pref(PrefPayload {
            category: "gcp".into(),
            key: "service_account".into(),
            value: serde_json::json!({
                "type": "service_account",
                "private_key": "-----BEGIN PRIVATE KEY-----\nMIIE...FAKE\n-----END PRIVATE KEY-----\n",
                "client_email": "robot@example.iam.gserviceaccount.com",
            }),
            previous_value: None,
        });
        // Either name is acceptable — both catch real leakage.
        let hit = detect_secret(&p).expect("must reject a service-account blob");
        assert!(
            hit == "gcp-service-account" || hit == "pem-private-key",
            "unexpected secret kind: {hit}"
        );
    }

    #[test]
    fn detects_gcp_blob_embedded_in_string_field() {
        // Codex R11 P1 #1: when the service-account JSON gets pasted into
        // a Rust String (Observe.content here), serde_json escapes the
        // inner quotes as \". The gcp-service-account regex that looks
        // for literal `"type":"service_account"` no longer matches the
        // escaped form. The pem-private-key pattern is the backstop —
        // `-----BEGIN PRIVATE KEY-----` contains no quote chars, so it
        // survives JSON escaping and trips on the escaped blob. This
        // test proves the backstop fires.
        let escaped_blob = r#"{
            "type": "service_account",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMIIE...\n-----END PRIVATE KEY-----\n",
            "client_email": "robot@example.iam.gserviceaccount.com"
        }"#;
        let p = EventPayload::Observe(ObservePayload {
            summary: "pasting a service account".into(),
            content: Some(escaped_blob.to_string()),
            content_ref: None,
            source: None,
        });
        // When the event serializes, the inner `"type"` becomes `\"type\"`
        // inside the enclosing `content` string — gcp-service-account
        // MISSES. pem-private-key MUST catch it.
        assert_eq!(detect_secret(&p), Some("pem-private-key"));
    }

    #[test]
    fn detects_labeled_twilio_auth_token() {
        // Codex R11 P1 #2: the account-SID pattern alone missed the
        // common env-var / config-file leak shape for the 32-hex auth
        // token. The labeled pattern fills this gap.
        let p = obs("TWILIO_AUTH_TOKEN=0123456789abcdef0123456789abcdef");
        assert_eq!(detect_secret(&p), Some("twilio-auth-token"));
    }

    #[test]
    fn detects_twilio_auth_token_in_json_field() {
        let p = obs(r#"config: { "twilio_auth_token": "abcdef0123456789abcdef0123456789" }"#);
        assert_eq!(detect_secret(&p), Some("twilio-auth-token"));
    }

    #[test]
    fn twilio_auth_token_requires_label() {
        // A bare 32-hex string must NOT match — too many false positives
        // (md5 sums, random hashes). Only trips when adjacent to the
        // `twilio_auth_token` label.
        let p = obs("the md5 sum is 0123456789abcdef0123456789abcdef");
        assert_eq!(detect_secret(&p), None);
    }
}
