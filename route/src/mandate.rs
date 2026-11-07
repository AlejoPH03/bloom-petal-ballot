//! The owner's standing rules for votes (docs/plan-mandate-inbox.md).
//!
//! **Advisory:** the mandate lives in the Petal's store and the agent can
//! rewrite it through `mandate.json`. It filters bad drafts before the owner
//! is asked; every vote still needs the owner's approval. An owner-signed,
//! binding mandate is future work.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::votes::body::sha256_hex;
use crate::votes::record::Binding;
use crate::votes::store::{KvStore, Scope, get_json, put_json};
use crate::{snapshot, validate_space};

pub const MANDATE_SCHEMA: &str = "ballot.mandate.v1";
pub const MARKER_SCHEMA: &str = "ballot.mandate-last-write.v1";
pub const MAX_BODY_BYTES: usize = 8192;
pub const MAX_SPACES: usize = 50;
pub const MAX_KEYWORDS: usize = 50;
pub const KEYWORD_MAX_CHARS: usize = 64;
pub const MAX_FOLLOW: usize = 50;
pub const MAX_VOTES_PER_DAY: u32 = 100;
/// The inbox reads at most this many mandate spaces.
pub const INBOX_MAX_SPACES: usize = 10;
/// The daily limit is a rolling window of this length.
pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;

pub const ADVISORY: &str = "The mandate is advisory: it is stored by this Petal and the agent can edit it. It filters bad drafts before the owner is asked; every vote still needs the owner's approval in a passkey ceremony. An owner-signed, binding mandate is future work.";

/// The stored mandate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mandate {
    pub schema: String,
    /// Allowed spaces; empty = no space restriction.
    pub spaces: Vec<String>,
    /// space -> delegate address (EIP-55). Stored only (following.json is
    /// future work).
    pub follow: BTreeMap<String, String>,
    pub blocked_keywords: Vec<String>,
    pub require_reason: bool,
    pub max_votes_per_day: Option<u32>,
    pub updated_ms: u64,
}

/// A parsed `mandate.json` write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MandateWrite {
    /// `{}`: delete the mandate.
    Clear,
    Set(Mandate),
}

fn strings(value: &Value, field: &str, max: usize) -> Result<Vec<String>, String> {
    let items = value.as_array().ok_or_else(|| format!("{field} must be an array of strings"))?;
    if items.len() > max {
        return Err(format!("{field} must have at most {max} entries"));
    }
    items
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or_else(|| format!("{field} must be an array of strings")))
        .collect()
}

impl MandateWrite {
    pub fn parse(body: &[u8], now_ms: u64) -> Result<Self, String> {
        if body.len() > MAX_BODY_BYTES {
            return Err(format!("write body must be at most {MAX_BODY_BYTES} bytes"));
        }
        let value: Value = serde_json::from_slice(body).map_err(|_| "write body must be a JSON object".to_owned())?;
        let Value::Object(map) = value else {
            return Err("write body must be a JSON object".into());
        };
        if map.is_empty() {
            return Ok(MandateWrite::Clear);
        }
        let mut mandate = Mandate {
            schema: MANDATE_SCHEMA.into(),
            spaces: Vec::new(),
            follow: BTreeMap::new(),
            blocked_keywords: Vec::new(),
            require_reason: false,
            max_votes_per_day: None,
            updated_ms: now_ms,
        };
        for (field, value) in &map {
            match field.as_str() {
                "spaces" => {
                    for space in strings(value, "spaces", MAX_SPACES)? {
                        validate_space(&space).map_err(|error| format!("spaces: {space:?}: {}", error.message()))?;
                        if !mandate.spaces.contains(&space) {
                            mandate.spaces.push(space);
                        }
                    }
                }
                "follow" => mandate.follow = parse_follow(value)?,
                "blocked_keywords" => {
                    for keyword in strings(value, "blocked_keywords", MAX_KEYWORDS)? {
                        let keyword = keyword.trim().to_owned();
                        let chars = keyword.chars().count();
                        if chars == 0 || chars > KEYWORD_MAX_CHARS {
                            return Err(format!("blocked_keywords entries must be 1-{KEYWORD_MAX_CHARS} characters"));
                        }
                        let lower = keyword.to_lowercase();
                        if !mandate.blocked_keywords.iter().any(|k| k.to_lowercase() == lower) {
                            mandate.blocked_keywords.push(keyword);
                        }
                    }
                }
                "require_reason" => {
                    mandate.require_reason = value.as_bool().ok_or("require_reason must be true or false")?;
                }
                "max_votes_per_day" => {
                    let n = value
                        .as_u64()
                        .filter(|n| (1..=u64::from(MAX_VOTES_PER_DAY)).contains(n))
                        .ok_or_else(|| format!("max_votes_per_day must be a whole number from 1 to {MAX_VOTES_PER_DAY}"))?;
                    mandate.max_votes_per_day = Some(n as u32);
                }
                other => {
                    return Err(format!(
                        "unknown field {:?}; allowed: spaces, follow, blocked_keywords, require_reason, max_votes_per_day",
                        crate::hub::sanitize(other)
                    ));
                }
            }
        }
        Ok(MandateWrite::Set(mandate))
    }
}

fn parse_follow(value: &Value) -> Result<BTreeMap<String, String>, String> {
    let map: &Map<String, Value> = value.as_object().ok_or("follow must be an object {space: delegate_address}")?;
    if map.len() > MAX_FOLLOW {
        return Err(format!("follow must have at most {MAX_FOLLOW} entries"));
    }
    let mut follow = BTreeMap::new();
    for (space, delegate) in map {
        validate_space(space).map_err(|error| format!("follow: {space:?}: {}", error.message()))?;
        let delegate = delegate.as_str().ok_or("follow values must be 0x addresses")?;
        if !delegate.starts_with("0x") {
            return Err(format!("follow: {space}: delegate must be 0x followed by 40 hex digits"));
        }
        let checksummed = snapshot::checksum_address(delegate).map_err(|reason| format!("follow: {space}: {reason}"))?;
        follow.insert(space.clone(), checksummed);
    }
    Ok(follow)
}

impl Mandate {
    /// Checked before any hub call.
    pub fn check_space(&self, space: &str) -> Result<(), (&'static str, String)> {
        if self.spaces.is_empty() || self.spaces.iter().any(|allowed| allowed == space) {
            return Ok(());
        }
        Err(("mandate_space_not_allowed", format!("space {space:?} is not in the mandate's spaces {:?}", self.spaces)))
    }

    /// Checked after the hub checks, in this order: keyword, reason, limit.
    /// `recent_votes` = this account's bound votes drafted in the last 24 h.
    pub fn check_draft(
        &self,
        title: &str,
        proposal_body: &str,
        reason: &str,
        recent_votes: u32,
    ) -> Result<(), (&'static str, String)> {
        let title_lower = title.to_lowercase();
        let body_lower = proposal_body.to_lowercase();
        for keyword in &self.blocked_keywords {
            let needle = keyword.to_lowercase();
            let place = if title_lower.contains(&needle) {
                Some("title")
            } else if body_lower.contains(&needle) {
                Some("body")
            } else {
                None
            };
            if let Some(place) = place {
                return Err((
                    "mandate_blocked_keyword",
                    format!("the proposal's {place} contains the blocked keyword {keyword:?}"),
                ));
            }
        }
        if self.require_reason && reason.is_empty() {
            return Err(("mandate_reason_required", "the mandate requires a non-empty reason".into()));
        }
        if let Some(limit) = self.max_votes_per_day
            && recent_votes >= limit
        {
            return Err((
                "mandate_daily_limit",
                format!("{recent_votes} votes drafted in the last 24 hours (rolling); the mandate allows {limit} per day"),
            ));
        }
        Ok(())
    }

    /// What the stored mandate means, for the read.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.spaces.is_empty() {
            notes.push("spaces is empty: votes in any space are allowed, and inbox.json has nothing to show".into());
        } else if self.spaces.len() > INBOX_MAX_SPACES {
            notes.push(format!("inbox.json reads only the first {INBOX_MAX_SPACES} spaces"));
        }
        match self.max_votes_per_day {
            Some(n) => notes.push(format!("at most {n} new votes per rolling 24 hours (bound votes, per network and account)")),
            None => notes.push("max_votes_per_day is not set: no daily limit".into()),
        }
        if !self.follow.is_empty() {
            notes.push("follow is stored only; nothing acts on it yet".into());
        }
        notes.push("the mandate applies to new vote_ids only: a vote already bound is never re-checked".into());
        notes
    }
}

pub fn mandate_key(scope: &Scope) -> String {
    format!("mandate/{}.json", scope.account_path())
}

pub fn marker_key(scope: &Scope) -> String {
    format!("markers/{}/mandate.json", scope.account_path())
}

pub fn load<H: KvStore>(store: &mut H, scope: &Scope) -> Result<Option<Mandate>, String> {
    get_json(store, &mandate_key(scope), "mandate")
}

/// This account's bound votes drafted after `now_ms - 24 h`.
pub fn recent_vote_count<H: KvStore>(store: &mut H, scope: &Scope, now_ms: u64) -> Result<u32, String> {
    let since = now_ms.saturating_sub(DAY_MS);
    let mut count = 0;
    for id in crate::votes::flow::list_vote_ids(store, scope)? {
        if let Some(binding) = get_json::<Binding>(store, &scope.binding_key(&id), "vote binding")?
            && binding.drafted_ms > since
        {
            count += 1;
        }
    }
    Ok(count)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MandateMarker {
    pub schema: String,
    pub at_ms: u64,
    pub body_sha256: String,
    pub outcome: String,
    pub code: i32,
    pub message: String,
}

/// The `mandate.json` write: `(code, message)`, 0 = success. Every outcome
/// is recorded in the marker.
pub fn write<H: KvStore>(store: &mut H, scope: &Scope, now_ms: Option<u64>, body: &[u8]) -> (i32, String) {
    let (code, outcome, message) = match now_ms.filter(|&ms| ms > 0) {
        None => (-4, "transient_error", "clock unavailable; retry the same write".to_owned()),
        Some(now_ms) => match MandateWrite::parse(body, now_ms) {
            Err(message) => (-3, "invalid_body", format!("invalid mandate: {message}")),
            Ok(MandateWrite::Clear) => match store.store_del(&mandate_key(scope)) {
                Ok(()) => (0, "cleared", String::new()),
                Err(error) => (-4, "transient_error", format!("store: {error}")),
            },
            Ok(MandateWrite::Set(mandate)) => match put_json(store, &mandate_key(scope), &mandate, "mandate") {
                Ok(()) => (0, "saved", String::new()),
                Err(error) => (-4, "transient_error", format!("store: {error}")),
            },
        },
    };
    let marker = MandateMarker {
        schema: MARKER_SCHEMA.into(),
        at_ms: now_ms.unwrap_or(0),
        body_sha256: sha256_hex(body),
        outcome: outcome.into(),
        code,
        message: message.clone(),
    };
    let _ = put_json(store, &marker_key(scope), &marker, "mandate marker");
    (code, message)
}

pub fn body_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "Every field is optional; a write replaces the whole mandate; {} clears it.",
        "properties": {
            "spaces": {"type": "array", "maxItems": MAX_SPACES, "items": {"type": "string", "pattern": "^[a-z0-9-]+(\\.[a-z0-9-]+)+$"}, "description": "allowed spaces; empty or absent = any space"},
            "follow": {"type": "object", "maxProperties": MAX_FOLLOW, "additionalProperties": {"type": "string", "pattern": "^0x[0-9a-fA-F]{40}$"}, "description": "space -> delegate address (stored only)"},
            "blocked_keywords": {"type": "array", "maxItems": MAX_KEYWORDS, "items": {"type": "string", "minLength": 1, "maxLength": KEYWORD_MAX_CHARS}, "description": "refuse proposals whose title or body contains one (case-insensitive)"},
            "require_reason": {"type": "boolean", "default": false},
            "max_votes_per_day": {"type": "integer", "minimum": 1, "maximum": MAX_VOTES_PER_DAY, "description": "rolling 24 h; absent = no limit"}
        },
        "max_bytes": MAX_BODY_BYTES
    })
}

/// The `mandate.json` read.
pub fn read_document<H: KvStore>(store: &mut H, scope: &Scope) -> Result<Value, String> {
    let mandate = load(store, scope)?;
    let marker: Option<MandateMarker> = get_json(store, &marker_key(scope), "mandate marker")?;
    let notes = match &mandate {
        Some(mandate) => mandate.notes(),
        None => vec!["no mandate: votes/new.json applies only its own checks; inbox.json needs mandate spaces".into()],
    };
    Ok(json!({
        "schema": "ballot.mandate-file.v1",
        "network": scope.network.as_str(),
        "wallet": scope.wallet,
        "index": scope.index,
        "advisory": ADVISORY,
        "mandate": mandate,
        "updated_ms": mandate.as_ref().map(|m| m.updated_ms),
        "notes": notes,
        "enforced_by": "votes/new.json, for new vote_ids: mandate_space_not_allowed (before any hub call), then after the hub checks mandate_blocked_keyword, mandate_reason_required, mandate_daily_limit. Each is a -3 refusal: nothing bound, no ceremony.",
        "write": format!("bloom vfs write {}/mandate.json --data '{{\"spaces\":[\"<space>\"],\"blocked_keywords\":[],\"require_reason\":false,\"max_votes_per_day\":5}}'", scope.account_dir()),
        "body_schema": body_schema(),
        "last_write": marker,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Network;
    use crate::votes::store::fake::MemStore;

    fn scope() -> Scope {
        Scope::new(Network::Testnet, "main", 0)
    }

    fn parse(text: &str) -> Result<MandateWrite, String> {
        MandateWrite::parse(text.as_bytes(), 5)
    }

    fn set(text: &str) -> Mandate {
        match parse(text).unwrap() {
            MandateWrite::Set(mandate) => mandate,
            MandateWrite::Clear => panic!("expected a mandate"),
        }
    }

    #[test]
    fn parses_the_full_body_and_normalises_it() {
        let m = set(r#"{"spaces":["bloom-ballot-182c.eth","ens.eth","ens.eth"],
            "follow":{"ens.eth":"0x042abe4ee65cbc71bd11297dcea2d69e1650a596"},
            "blocked_keywords":["  treasury transfer ","Treasury Transfer","mint"],
            "require_reason":true,"max_votes_per_day":5}"#);
        assert_eq!(m.spaces, ["bloom-ballot-182c.eth", "ens.eth"], "duplicates dropped, order kept");
        assert_eq!(m.follow["ens.eth"], "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596", "checksummed");
        assert_eq!(m.blocked_keywords, ["treasury transfer", "mint"], "trimmed, case-insensitive dedupe");
        assert!(m.require_reason);
        assert_eq!(m.max_votes_per_day, Some(5));
        assert_eq!(m.updated_ms, 5);
        let partial = set(r#"{"require_reason":true}"#);
        assert!(partial.spaces.is_empty() && partial.max_votes_per_day.is_none());
    }

    #[test]
    fn empty_object_clears() {
        assert_eq!(parse("{}").unwrap(), MandateWrite::Clear);
        assert_eq!(parse(" { }\n").unwrap(), MandateWrite::Clear);
        assert!(matches!(parse(r#"{"spaces":[]}"#).unwrap(), MandateWrite::Set(_)));
    }

    #[test]
    fn rejects_bad_bodies() {
        let many_spaces = format!(r#"{{"spaces":[{}]}}"#, (0..51).map(|i| format!("\"s{i}.eth\"")).collect::<Vec<_>>().join(","));
        let many_keywords = format!(r#"{{"blocked_keywords":[{}]}}"#, (0..51).map(|i| format!("\"k{i}\"")).collect::<Vec<_>>().join(","));
        let long_keyword = format!(r#"{{"blocked_keywords":["{}"]}}"#, "k".repeat(65));
        let too_big = format!(r#"{{"blocked_keywords":["{}"]}}"#, "k".repeat(MAX_BODY_BYTES));
        let cases: Vec<(String, &str)> = vec![
            ("[]".into(), "JSON object"),
            ("nope".into(), "JSON object"),
            (r#"{"spaces":["ens.eth"],"extra":1}"#.into(), "unknown field \"extra\""),
            (r#"{"spaces":"ens.eth"}"#.into(), "array of strings"),
            (r#"{"spaces":["ENS.eth"]}"#.into(), "spaces: \"ENS.eth\""),
            (many_spaces, "at most 50"),
            (r#"{"blocked_keywords":["  "]}"#.into(), "1-64 characters"),
            (long_keyword, "1-64 characters"),
            (many_keywords, "at most 50"),
            (r#"{"follow":{"ens.eth":"042abe4ee65cbc71bd11297dcea2d69e1650a596"}}"#.into(), "0x followed by 40"),
            (r#"{"follow":{"ens.eth":"0x123"}}"#.into(), "40 hex"),
            (r#"{"follow":{"bad":"0x042abe4ee65cbc71bd11297dcea2d69e1650a596"}}"#.into(), "follow: \"bad\""),
            (r#"{"follow":[]}"#.into(), "object"),
            (r#"{"require_reason":"yes"}"#.into(), "true or false"),
            (r#"{"max_votes_per_day":0}"#.into(), "1 to 100"),
            (r#"{"max_votes_per_day":101}"#.into(), "1 to 100"),
            (r#"{"max_votes_per_day":2.5}"#.into(), "1 to 100"),
            (too_big, "at most 8192 bytes"),
        ];
        for (body, needle) in cases {
            let error = parse(&body).unwrap_err();
            assert!(error.contains(needle), "{body:.80} -> {error} (want {needle})");
        }
        let max_keyword = format!(r#"{{"blocked_keywords":["{}"]}}"#, "é".repeat(64));
        assert!(parse(&max_keyword).is_ok(), "64 characters, more bytes");
    }

    #[test]
    fn space_check_allows_listed_spaces_or_any_when_empty() {
        let m = set(r#"{"spaces":["bloom-ballot-182c.eth"]}"#);
        assert!(m.check_space("bloom-ballot-182c.eth").is_ok());
        let (code, detail) = m.check_space("ens.eth").unwrap_err();
        assert_eq!(code, "mandate_space_not_allowed");
        assert!(detail.contains("\"ens.eth\""));
        assert!(set(r#"{"require_reason":true}"#).check_space("ens.eth").is_ok());
    }

    #[test]
    fn draft_checks_run_in_order_keyword_reason_limit() {
        let m = set(r#"{"blocked_keywords":["Treasury Transfer"],"require_reason":true,"max_votes_per_day":2}"#);
        let (code, detail) = m.check_draft("Treasury transfer to grants multisig", "", "", 9).unwrap_err();
        assert_eq!(code, "mandate_blocked_keyword", "keyword first");
        assert_eq!(detail, "the proposal's title contains the blocked keyword \"Treasury Transfer\"");
        let (code, detail) = m.check_draft("Fee change", "We propose a TREASURY TRANSFER of 5 ETH", "", 0).unwrap_err();
        assert_eq!(code, "mandate_blocked_keyword");
        assert!(detail.contains("body"));
        assert_eq!(m.check_draft("Fee change", "", "", 9).unwrap_err().0, "mandate_reason_required");
        assert_eq!(m.check_draft("Fee change", "", "because", 2).unwrap_err().0, "mandate_daily_limit");
        assert!(m.check_draft("Fee change", "", "because", 1).is_ok());
        assert!(set(r#"{"spaces":["a.eth"]}"#).check_draft("x", "", "", 1000).is_ok(), "no limit set");
    }

    #[test]
    fn notes_explain_the_mandate() {
        let notes = set(r#"{"require_reason":true}"#).notes().join(" | ");
        assert!(notes.contains("any space is allowed") || notes.contains("votes in any space are allowed"));
        assert!(notes.contains("no daily limit"));
        let many = format!(r#"{{"spaces":[{}],"follow":{{"a.eth":"0x042abe4ee65cbc71bd11297dcea2d69e1650a596"}},"max_votes_per_day":3}}"#,
            (0..11).map(|i| format!("\"s{i}.eth\"")).collect::<Vec<_>>().join(","));
        let notes = set(&many).notes().join(" | ");
        assert!(notes.contains("first 10 spaces") && notes.contains("follow is stored only") && notes.contains("at most 3"));
    }

    #[test]
    fn write_saves_clears_and_records_every_outcome() {
        let mut store = MemStore::default();
        let s = scope();
        assert_eq!(write(&mut store, &s, Some(10), br#"{"spaces":["bloom-ballot-182c.eth"],"max_votes_per_day":5}"#), (0, String::new()));
        let saved = load(&mut store, &s).unwrap().unwrap();
        assert_eq!((saved.max_votes_per_day, saved.updated_ms), (Some(5), 10));
        let doc = read_document(&mut store, &s).unwrap();
        assert_eq!(doc["mandate"]["spaces"][0], "bloom-ballot-182c.eth");
        assert_eq!(doc["updated_ms"], 10);
        assert_eq!(doc["last_write"]["outcome"], "saved");
        assert!(doc["advisory"].as_str().unwrap().contains("advisory"));

        let (code, message) = write(&mut store, &s, Some(11), br#"{"bad":1}"#);
        assert_eq!(code, -3);
        assert!(message.starts_with("invalid mandate: unknown field"));
        assert_eq!(load(&mut store, &s).unwrap(), Some(saved), "an invalid write changes nothing");
        let doc = read_document(&mut store, &s).unwrap();
        assert_eq!((doc["last_write"]["outcome"].as_str(), doc["last_write"]["message"].as_str()), (Some("invalid_body"), Some(message.as_str())));

        assert_eq!(write(&mut store, &s, Some(12), b"{}"), (0, String::new()));
        assert_eq!(load(&mut store, &s).unwrap(), None);
        let doc = read_document(&mut store, &s).unwrap();
        assert_eq!(doc["mandate"], Value::Null);
        assert_eq!(doc["last_write"]["outcome"], "cleared");
        assert_eq!(write(&mut store, &s, Some(13), b"{}").0, 0, "clearing twice is fine");

        assert_eq!(write(&mut store, &s, None, b"{}").0, -4);
        let mut broken = MemStore { writes_left: Some(0), ..MemStore::default() };
        assert_eq!(write(&mut broken, &s, Some(1), br#"{"require_reason":true}"#).0, -4);
    }

    #[test]
    fn mandates_are_per_network_and_account() {
        let mut store = MemStore::default();
        write(&mut store, &scope(), Some(1), br#"{"require_reason":true}"#);
        assert!(load(&mut store, &Scope::new(Network::Mainnet, "main", 0)).unwrap().is_none());
        assert!(load(&mut store, &Scope::new(Network::Testnet, "main", 1)).unwrap().is_none());
        assert_eq!(mandate_key(&scope()), "mandate/testnet/main/0.json");
        assert_eq!(marker_key(&scope()), "markers/testnet/main/0/mandate.json");
    }

    #[test]
    fn recent_votes_count_bindings_in_a_rolling_day() {
        let mut store = MemStore::default();
        let s = scope();
        // Only drafted_ms matters to the count.
        let full: Binding = serde_json::from_value(json!({
            "schema": "ballot.vote-binding.v1", "vote_id": "x", "network": "testnet", "wallet": "main", "index": 0,
            "canonical_body_sha256": "", "from": "0x0", "space": "a.eth", "proposal": "0x", "choice": 1, "reason": "",
            "timestamp": 0, "app": "bloom-ballot", "metadata": "{}",
            "proposal_facts": {"title": "", "type": "single-choice", "choices": [], "choice_label": "", "start": 0, "end": 0},
            "typed_data": null, "preimage": "", "digest": "", "drafted_ms": 0
        })).unwrap();
        let now = 10 * DAY_MS;
        for (id, drafted) in [("old", now - DAY_MS), ("edge", now - DAY_MS + 1), ("new", now - 1)] {
            let mut b = full.clone();
            b.drafted_ms = drafted;
            put_json(&mut store, &s.binding_key(id), &b, "b").unwrap();
        }
        // A refused vote has a record but no binding: it never counts.
        put_json(&mut store, &s.record_key("refused"), &json!({}), "r").unwrap();
        assert_eq!(recent_vote_count(&mut store, &s, now).unwrap(), 2, "exactly 24 h ago is outside the window");
    }
}
