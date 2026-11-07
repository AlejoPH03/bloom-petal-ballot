//! Snapshot sequencer envelope and response classification
//! (docs/plan-votes.md §4).
//!
//! Sources (snapshot-sequencer `f577889`, deployed `14783ac`):
//! - envelope `{address, sig, data:{domain, types, message}}` (snapshot.js
//!   `sign()`, FINDINGS.md §2); `types` without `EIP712Domain`, hashed by the
//!   sequencer as `sha256(JSON.stringify(types))` (`src/ingestor.ts:76-81`);
//! - success `{id, ipfs, relayer:{address, receipt}}` (`ingestor.ts:284-291`);
//! - errors `{error, error_description}`, 400 for string rejections, 500
//!   otherwise, 503 maintenance (`src/api.ts:19-36`, `helpers/utils.ts:33-39`),
//!   425 for a concurrent identical body (`helpers/duplicateRequestPreventor.ts:8-19`).

use serde::Deserialize;
use serde_json::{Value, json};

use crate::snapshot::{DOMAIN_NAME, DOMAIN_VERSION, Vote};
use crate::votes::body::sha256_hex;

/// Largest sequencer response body accepted.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024;

/// Signature bytes. `Debug` never prints them, so they cannot reach a test
/// failure message or a log through formatting.
#[derive(Clone, PartialEq, Eq)]
pub struct SignatureBytes(Vec<u8>);

impl SignatureBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// The public fingerprint (`snapshot::signature_fingerprint`).
    pub fn fingerprint(&self) -> String {
        crate::snapshot::signature_fingerprint(&self.0)
    }
}

impl std::fmt::Debug for SignatureBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignatureBytes(<{} bytes, redacted>)", self.0.len())
    }
}

/// The envelope `data` (no `EIP712Domain`, no `primaryType`), exactly as
/// snapshot.js sends it. serde_json sorts object keys, so each type entry is
/// `{"name","type"}` in that order, which is snapshot.js's order too.
pub fn envelope_data(vote: &Vote) -> Value {
    let typed = vote.typed_data();
    json!({
        "domain": {"name": DOMAIN_NAME, "version": DOMAIN_VERSION},
        "types": {"Vote": typed["types"]["Vote"].clone()},
        "message": typed["message"].clone(),
    })
}

/// The POST body. Built in memory only; never stored, logged or returned.
pub fn envelope_bytes(vote: &Vote, signature: &SignatureBytes) -> Vec<u8> {
    let envelope = json!({
        "address": vote.from,
        "sig": format!("0x{}", alloy_primitives::hex::encode(signature.as_slice())),
        "data": envelope_data(vote),
    });
    serde_json::to_vec(&envelope).unwrap_or_default()
}

/// What a POST returned, as the host saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostResult {
    Http { status: u16, body: Vec<u8> },
    /// The request did not complete (net policy, timeout, oversized body…).
    Transport(String),
}

/// A sequencer acceptance. The relayer receipt is a signature, so only its
/// sha256 is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub id: String,
    pub ipfs: String,
    pub relayer_address: String,
    pub receipt_sha256: Option<String>,
}

/// Classified sequencer outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Submission {
    Accepted(Accepted),
    /// The id already exists (`duplicate message`): check the hub.
    Duplicate,
    /// Retry the same write later; the record stays `signed`.
    Transient(String),
    /// The sequencer refused this vote for good.
    Permanent(Rejection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    NotInVotingWindow,
    WrongTimestamp,
    NewerVoteExists,
    WrongEnvelopeFormat,
    TooLargeMessage,
    ProposalWhitespace,
    WrongDomain,
    WrongTypes,
    InvalidSpaceId,
    UnknownSpace,
    SignatureValidationFailed,
    TooLargeMetadata,
    WrongVoteFormat,
    UnknownProposal,
    InvalidChoice,
    FailedVoteValidation,
    NoVotingPower,
}

impl Rejection {
    pub fn code(self) -> &'static str {
        match self {
            Rejection::NotInVotingWindow => "not_in_voting_window",
            Rejection::WrongTimestamp => "wrong_timestamp",
            Rejection::NewerVoteExists => "newer_vote_exists",
            Rejection::WrongEnvelopeFormat => "wrong_envelope_format",
            Rejection::TooLargeMessage => "too_large_message",
            Rejection::ProposalWhitespace => "proposal_whitespace",
            Rejection::WrongDomain => "wrong_domain",
            Rejection::WrongTypes => "wrong_types",
            Rejection::InvalidSpaceId => "invalid_space_id",
            Rejection::UnknownSpace => "unknown_space",
            Rejection::SignatureValidationFailed => "signature_validation_failed",
            Rejection::TooLargeMetadata => "too_large_metadata",
            Rejection::WrongVoteFormat => "wrong_vote_format",
            Rejection::UnknownProposal => "unknown_proposal",
            Rejection::InvalidChoice => "invalid_choice",
            Rejection::FailedVoteValidation => "failed_vote_validation",
            Rejection::NoVotingPower => "no_voting_power",
        }
    }
}

/// Map one `error_description` to its class (plan §4 table).
pub fn classify_error(status: u16, description: &str) -> Submission {
    use Rejection::*;
    let permanent = match description {
        "duplicate message" => return Submission::Duplicate,
        // ingestor.ts:47-117
        "wrong envelope format" => WrongEnvelopeFormat,
        "too large message" => TooLargeMessage,
        "wrong timestamp" => WrongTimestamp,
        "proposal cannot contain whitespace" => ProposalWhitespace,
        "wrong domain" => WrongDomain,
        "wrong types" => WrongTypes,
        "Invalid space id" => InvalidSpaceId,
        "unknown space" => UnknownSpace,
        "signature validation failed" => SignatureValidationFailed,
        // ingestor.ts:173-174
        "too large metadata" => TooLargeMetadata,
        // writer/vote.ts:21-151
        "wrong vote format" => WrongVoteFormat,
        "unknown proposal" => UnknownProposal,
        "not in voting window" => NotInVotingWindow,
        "invalid choice" => InvalidChoice,
        "failed vote validation" => FailedVoteValidation,
        "no voting power" => NoVotingPower,
        "already voted at later time" | "already voted same time with lower index" => NewerVoteExists,
        // 425, 503, pinning failed, failed to check …, 5xx and anything else.
        other => {
            let text = if other.is_empty() { format!("status {status}") } else { redact(other) };
            return Submission::Transient(format!("snapshot sequencer: {text}"));
        }
    };
    Submission::Permanent(permanent)
}

#[derive(Deserialize)]
struct Success {
    id: String,
    #[serde(default)]
    ipfs: String,
    #[serde(default)]
    relayer: Option<Relayer>,
}

#[derive(Deserialize)]
struct Relayer {
    #[serde(default)]
    address: String,
    #[serde(default)]
    receipt: Option<String>,
}

#[derive(Deserialize)]
struct Failure {
    #[serde(default)]
    error_description: Option<Value>,
}

/// Classify what the POST returned.
pub fn classify(result: &PostResult) -> Submission {
    let (status, body) = match result {
        PostResult::Transport(message) => {
            return Submission::Transient(format!("snapshot sequencer request failed: {}", redact(message)));
        }
        PostResult::Http { status, body } => (*status, body),
    };
    if body.len() > MAX_RESPONSE_BYTES {
        return Submission::Transient("snapshot sequencer response is too large".into());
    }
    if (200..300).contains(&status) {
        return match serde_json::from_slice::<Success>(body) {
            Ok(success) if !success.id.is_empty() => {
                let relayer = success.relayer;
                Submission::Accepted(Accepted {
                    id: success.id,
                    ipfs: success.ipfs,
                    relayer_address: relayer.as_ref().map(|r| r.address.clone()).unwrap_or_default(),
                    receipt_sha256: relayer.and_then(|r| r.receipt).map(|receipt| sha256_hex(receipt.as_bytes())),
                })
            }
            _ => Submission::Transient(format!("snapshot sequencer returned an unexpected {status} response")),
        };
    }
    let description = serde_json::from_slice::<Failure>(body)
        .ok()
        .and_then(|failure| failure.error_description)
        .map(|value| match value {
            Value::String(text) => text,
            other => other.to_string(),
        })
        .unwrap_or_default();
    classify_error(status, &description)
}

/// Make upstream text safe to store or return: any run of 130 or more hex
/// digits (a 65-byte signature) is replaced, then `hub::sanitize` applies
/// (printable, URLs replaced, at most 160 bytes).
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() >= 130 {
            out.push_str("[redacted]");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_hexdigit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    crate::hub::sanitize(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! golden {
        ($name:literal) => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/snapshot-testnet/golden/", $name))
        };
    }

    /// Exactly the string FINDINGS.md:56-57 / golden/build_vector.sh hash.
    const VOTE_TYPES_JSON: &str = r#"{"Vote":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"proposal","type":"string"},{"name":"choice","type":"uint32"},{"name":"reason","type":"string"},{"name":"app","type":"string"},{"name":"metadata","type":"string"}]}"#;

    fn golden_vote() -> Vote {
        let m: Value = serde_json::from_str(golden!("message.json")).unwrap();
        Vote {
            from: m["from"].as_str().unwrap().into(),
            space: m["space"].as_str().unwrap().into(),
            timestamp: m["timestamp"].as_u64().unwrap(),
            proposal: m["proposal"].as_str().unwrap().into(),
            choice: m["choice"].as_u64().unwrap() as u32,
            reason: m["reason"].as_str().unwrap().into(),
            app: m["app"].as_str().unwrap().into(),
            metadata: m["metadata"].as_str().unwrap().into(),
        }
    }

    fn golden_signature() -> SignatureBytes {
        let hex = golden!("signature.hex").trim();
        SignatureBytes::new(alloy_primitives::hex::decode(hex).unwrap())
    }

    #[test]
    fn envelope_matches_the_golden_envelope() {
        let bytes = envelope_bytes(&golden_vote(), &golden_signature());
        let ours: Value = serde_json::from_slice(&bytes).unwrap();
        let golden: Value = serde_json::from_str(golden!("envelope.json")).unwrap();
        // Compare without printing: the envelope carries a signature.
        assert!(ours == golden, "envelope differs from golden/envelope.json");
        let data: Value = serde_json::from_str(golden!("envelope_data.json")).unwrap();
        assert_eq!(envelope_data(&golden_vote()), data);
        assert!(ours["data"].get("primaryType").is_none());
        assert!(ours["data"]["types"].get("EIP712Domain").is_none());
    }

    #[test]
    fn types_serialise_exactly_as_the_sequencer_hashes_them() {
        let types = serde_json::to_string(&envelope_data(&golden_vote())["types"]).unwrap();
        assert_eq!(types, VOTE_TYPES_JSON);
        assert_eq!(
            sha256_hex(types.as_bytes()),
            "fb8fa9816cd42974e7f1af671aa548c8c458553364ed809e45042f141de8c0d5",
            "snapshot.js hashedTypes.json:73 = vote"
        );
    }

    #[test]
    fn signature_debug_is_redacted() {
        let text = format!("{:?}", golden_signature());
        assert_eq!(text, "SignatureBytes(<65 bytes, redacted>)");
    }

    #[test]
    fn success_keeps_only_public_identifiers() {
        let body = br#"{"id":"0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657","ipfs":"bafkreigs5zwiono6neqpkz5ye5py757jzge3j4ftguigcuwi6zbx6dhi74","relayer":{"address":"0x8BBE4Ac64246d600BC2889ef5d83809D138F03DF","receipt":"0x2eb06d05"}}"#;
        let Submission::Accepted(accepted) = classify(&PostResult::Http { status: 200, body: body.to_vec() }) else {
            panic!("expected accepted");
        };
        assert_eq!(accepted.id, "0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657");
        assert_eq!(accepted.relayer_address, "0x8BBE4Ac64246d600BC2889ef5d83809D138F03DF");
        assert_eq!(accepted.receipt_sha256, Some(sha256_hex(b"0x2eb06d05")));
        let odd = classify(&PostResult::Http { status: 200, body: b"{}".to_vec() });
        assert!(matches!(odd, Submission::Transient(_)));
    }

    #[test]
    fn every_documented_error_is_classified() {
        let permanent = [
            ("wrong envelope format", Rejection::WrongEnvelopeFormat),
            ("too large message", Rejection::TooLargeMessage),
            ("wrong timestamp", Rejection::WrongTimestamp),
            ("proposal cannot contain whitespace", Rejection::ProposalWhitespace),
            ("wrong domain", Rejection::WrongDomain),
            ("wrong types", Rejection::WrongTypes),
            ("Invalid space id", Rejection::InvalidSpaceId),
            ("unknown space", Rejection::UnknownSpace),
            ("signature validation failed", Rejection::SignatureValidationFailed),
            ("too large metadata", Rejection::TooLargeMetadata),
            ("wrong vote format", Rejection::WrongVoteFormat),
            ("unknown proposal", Rejection::UnknownProposal),
            ("not in voting window", Rejection::NotInVotingWindow),
            ("invalid choice", Rejection::InvalidChoice),
            ("failed vote validation", Rejection::FailedVoteValidation),
            ("no voting power", Rejection::NoVotingPower),
            ("already voted at later time", Rejection::NewerVoteExists),
            ("already voted same time with lower index", Rejection::NewerVoteExists),
        ];
        for (text, want) in permanent {
            let body = json!({"error": "client_error", "error_description": text}).to_string().into_bytes();
            assert_eq!(classify(&PostResult::Http { status: 400, body }), Submission::Permanent(want), "{text}");
        }
        let dup = json!({"error": "client_error", "error_description": "duplicate message"}).to_string();
        assert_eq!(classify(&PostResult::Http { status: 400, body: dup.into_bytes() }), Submission::Duplicate);
        for (status, text) in [
            (425, "request already being processed"),
            (503, "update in progress, try later"),
            (400, "pinning failed"),
            (400, "failed to check vote validation"),
            (400, "failed to check voting power"),
            (500, ""),
            (502, "something new"),
        ] {
            let body = json!({"error": "x", "error_description": text}).to_string().into_bytes();
            assert!(matches!(classify(&PostResult::Http { status, body }), Submission::Transient(_)), "{status} {text}");
        }
        for body in [b"<html>bad gateway</html>".to_vec(), vec![b'x'; MAX_RESPONSE_BYTES + 1], vec![]] {
            assert!(matches!(classify(&PostResult::Http { status: 502, body }), Submission::Transient(_)));
        }
        assert!(matches!(classify(&PostResult::Transport("timed out".into())), Submission::Transient(_)));
        // A non-string description (an object from an unexpected exception).
        let obj = json!({"error": "server_error", "error_description": {"code": 1}}).to_string().into_bytes();
        assert!(matches!(classify(&PostResult::Http { status: 500, body: obj }), Submission::Transient(_)));
    }

    #[test]
    fn redaction_removes_signature_sized_hex_runs() {
        let sig = "ab".repeat(65);
        let text = redact(&format!("bad sig 0x{sig} for 0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657"));
        assert!(!text.contains(&sig));
        assert!(text.contains("0x[redacted]"));
        assert!(text.contains("d74075ee"), "a 64-hex digest is public and kept");
        let transient = classify_error(500, &format!("oops {sig}"));
        let Submission::Transient(message) = transient else { panic!() };
        assert!(!message.contains(&sig));
    }
}
