//! The `votes/new.json` write body (docs/plan-votes.md §3, step 1).
//!
//! `{"vote_id", "space", "proposal", "choice", "reason"?}`, at most 4096
//! bytes, unknown fields refused. Identity is the normalised tuple
//! `(space, proposal, choice, reason)` with a missing `reason` equal to `""`
//! (snapshot.js's default, FINDINGS.md §2): whitespace and key order do not
//! matter.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{validate_proposal_id, validate_space, validate_vote_id};

pub const MAX_BODY_BYTES: usize = 4096;
/// Longest accepted `reason`, in Unicode scalar values (Snapshot allows 5000,
/// snapshot.js `src/schemas/vote.json`; FINDINGS.md §3).
pub const REASON_MAX_CHARS: usize = 500;
/// `votes/new.json` is a literal route that always wins over
/// `votes/[vote_id].json` (`bloom-petals/src/package.rs:1569-1585`), so a vote
/// called `new` could never be read.
pub const RESERVED_VOTE_ID: &str = "new";

const SHAPE: &str = r#"write body must be a JSON object {"vote_id", "space", "proposal", "choice", "reason"?} with no other fields"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBody {
    vote_id: String,
    space: String,
    proposal: String,
    choice: u32,
    #[serde(default)]
    reason: Option<String>,
}

/// A validated, normalised vote request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteBody {
    pub vote_id: String,
    pub space: String,
    pub proposal: String,
    pub choice: u32,
    pub reason: String,
}

/// Validate a vote id for use as a store key segment and file name.
pub fn validate_route_vote_id(value: &str) -> Result<&str, &'static str> {
    let value = validate_vote_id(value).map_err(|error| error.message())?;
    if value == RESERVED_VOTE_ID {
        return Err("vote id \"new\" is reserved (votes/new.json)");
    }
    Ok(value)
}

impl VoteBody {
    pub fn parse(body: &[u8]) -> Result<Self, String> {
        if body.len() > MAX_BODY_BYTES {
            return Err(format!("write body must be at most {MAX_BODY_BYTES} bytes"));
        }
        // serde would also accept a JSON array for a struct; require an object.
        let value: Value = serde_json::from_slice(body).map_err(|_| SHAPE.to_owned())?;
        if !value.is_object() {
            return Err(SHAPE.into());
        }
        if let Some(choice) = value.get("choice")
            && !choice.as_u64().is_some_and(|n| u32::try_from(n).is_ok())
        {
            return Err("choice must be a whole number from 1 to 4294967295".into());
        }
        let raw: RawBody = serde_json::from_value(value).map_err(|error| {
            let text = error.to_string();
            if text.contains("unknown field") || text.contains("missing field") {
                format!("{SHAPE}: {}", crate::hub::sanitize(&text))
            } else {
                SHAPE.to_owned()
            }
        })?;
        validate_route_vote_id(&raw.vote_id)?;
        validate_space(&raw.space).map_err(|error| error.message().to_owned())?;
        validate_proposal_id(&raw.proposal).map_err(|error| error.message().to_owned())?;
        if raw.choice == 0 {
            return Err("choice must be at least 1 (Snapshot numbers choices from 1)".into());
        }
        let reason = raw.reason.unwrap_or_default();
        if reason.chars().count() > REASON_MAX_CHARS {
            return Err(format!("reason must be at most {REASON_MAX_CHARS} characters"));
        }
        Ok(Self { vote_id: raw.vote_id, space: raw.space, proposal: raw.proposal, choice: raw.choice, reason })
    }

    /// Whether `other` is the same vote: equal space, proposal, choice and
    /// (normalised) reason.
    pub fn same_vote(&self, space: &str, proposal: &str, choice: u32, reason: &str) -> bool {
        self.space == space && self.proposal == proposal && self.choice == choice && self.reason == reason
    }

    /// sha256 of the normalised body (compact JSON, sorted keys).
    pub fn canonical_sha256(&self) -> String {
        let canonical = json!({
            "choice": self.choice,
            "proposal": self.proposal,
            "reason": self.reason,
            "space": self.space,
            "vote_id": self.vote_id,
        });
        sha256_hex(&serde_json::to_vec(&canonical).unwrap_or_default())
    }
}

/// Best-effort vote id of a body that failed validation (for the marker), if
/// it carries a valid one.
pub fn vote_id_hint(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let id = value.get("vote_id")?.as_str()?;
    validate_route_vote_id(id).ok().map(str::to_owned)
}

/// Lowercase hex sha256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    alloy_primitives::hex::encode(Sha256::digest(bytes))
}

/// The machine-readable body schema shown by `new.json`'s read.
pub fn body_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["vote_id", "space", "proposal", "choice"],
        "properties": {
            "vote_id": {"type": "string", "pattern": "^[a-z0-9-]{1,48}$", "not": {"const": RESERVED_VOTE_ID},
                        "description": "your id for this vote; binds permanently to this vote once drafted"},
            "space": {"type": "string", "pattern": "^[a-z0-9-]+(\\.[a-z0-9-]+)+$", "maxLength": crate::SPACE_MAX_LEN},
            "proposal": {"type": "string", "pattern": "^0x[0-9a-f]{64}$"},
            "choice": {"type": "integer", "minimum": 1, "description": "1-based choice number (see the proposal's choices)"},
            "reason": {"type": "string", "maxLength": REASON_MAX_CHARS, "default": ""}
        },
        "max_bytes": MAX_BODY_BYTES
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPOSAL: &str = "0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00";

    fn body(extra: &str) -> String {
        format!(r#"{{"vote_id":"v-1","space":"bloom-ballot-182c.eth","proposal":"{PROPOSAL}","choice":1{extra}}}"#)
    }

    #[test]
    fn parses_and_normalises_a_minimal_body() {
        let parsed = VoteBody::parse(body("").as_bytes()).unwrap();
        assert_eq!(parsed.vote_id, "v-1");
        assert_eq!(parsed.choice, 1);
        assert_eq!(parsed.reason, "", "missing reason is the empty string");
        let with_empty = VoteBody::parse(body(r#","reason":"""#).as_bytes()).unwrap();
        assert_eq!(parsed, with_empty);
        assert_eq!(parsed.canonical_sha256(), with_empty.canonical_sha256());
        // Whitespace, key order and a trailing newline do not matter.
        let reordered = format!(
            "{{ \"choice\": 1,\n \"proposal\": \"{PROPOSAL}\", \"space\": \"bloom-ballot-182c.eth\", \"vote_id\": \"v-1\" }}\n"
        );
        assert_eq!(VoteBody::parse(reordered.as_bytes()).unwrap(), parsed);
        assert!(parsed.same_vote("bloom-ballot-182c.eth", PROPOSAL, 1, ""));
        assert!(!parsed.same_vote("bloom-ballot-182c.eth", PROPOSAL, 2, ""));
        assert!(!parsed.same_vote("bloom-ballot-182c.eth", PROPOSAL, 1, "x"));
    }

    #[test]
    fn size_limit_is_4096_bytes() {
        let base = body(r#","reason":""#);
        let pad = |n: usize| format!("{base}{}\"}}", "a".repeat(n));
        let fits = pad(MAX_BODY_BYTES - base.len() - 2);
        assert_eq!(fits.len(), MAX_BODY_BYTES);
        // 4096 bytes parse the size check (the reason itself is too long).
        assert_eq!(VoteBody::parse(fits.as_bytes()).unwrap_err(), "reason must be at most 500 characters");
        let over = pad(MAX_BODY_BYTES - base.len() - 1);
        assert_eq!(VoteBody::parse(over.as_bytes()).unwrap_err(), "write body must be at most 4096 bytes");
    }

    #[test]
    fn reason_limit_counts_characters_not_bytes() {
        let ok = body(&format!(r#","reason":"{}""#, "é".repeat(REASON_MAX_CHARS)));
        assert!(ok.len() > REASON_MAX_CHARS + 100, "multibyte: more bytes than characters");
        assert_eq!(VoteBody::parse(ok.as_bytes()).unwrap().reason.chars().count(), 500);
        let long = body(&format!(r#","reason":"{}""#, "é".repeat(REASON_MAX_CHARS + 1)));
        assert!(VoteBody::parse(long.as_bytes()).is_err());
    }

    #[test]
    fn rejects_bad_bodies() {
        let cases: Vec<(String, &str)> = vec![
            ("[]".into(), "JSON object"),
            ("hello".into(), "JSON object"),
            ("".into(), "JSON object"),
            (body(r#","extra":1"#), "unknown field"),
            (format!(r#"{{"vote_id":"v","space":"a.eth","proposal":"{PROPOSAL}"}}"#), "missing field"),
            (body("").replace(r#""choice":1"#, r#""choice":0"#), "at least 1"),
            (body("").replace(r#""choice":1"#, r#""choice":-1"#), "whole number"),
            (body("").replace(r#""choice":1"#, r#""choice":"1""#), "whole number"),
            (body("").replace(r#""choice":1"#, r#""choice":1.5"#), "whole number"),
            (body("").replace("\"v-1\"", "\"new\""), "reserved"),
            (body("").replace("\"v-1\"", "\"V-1\""), "[a-z0-9-]"),
            (body("").replace("\"v-1\"", &format!("\"{}\"", "a".repeat(49))), "1-48"),
            (body("").replace("bloom-ballot-182c.eth", "nodot"), "dot"),
            (body("").replace(PROPOSAL, &PROPOSAL.to_uppercase().replace("0X", "0x")), "lowercase"),
            (body(r#","reason":7"#), "JSON object"),
        ];
        for (input, needle) in cases {
            let error = VoteBody::parse(input.as_bytes()).unwrap_err();
            assert!(error.contains(needle), "{input:?} -> {error:?} (want {needle:?})");
        }
    }

    #[test]
    fn vote_id_hint_only_returns_valid_ids() {
        assert_eq!(vote_id_hint(br#"{"vote_id":"ok-1","x":2}"#).as_deref(), Some("ok-1"));
        assert_eq!(vote_id_hint(br#"{"vote_id":"new"}"#), None);
        assert_eq!(vote_id_hint(br#"{"vote_id":"../x"}"#), None);
        assert_eq!(vote_id_hint(b"nope"), None);
    }

    #[test]
    fn canonical_hash_distinguishes_votes() {
        let a = VoteBody::parse(body("").as_bytes()).unwrap();
        let b = VoteBody::parse(body(r#","reason":"r""#).as_bytes()).unwrap();
        assert_ne!(a.canonical_sha256(), b.canonical_sha256());
        assert_eq!(a.canonical_sha256().len(), 64);
    }
}
