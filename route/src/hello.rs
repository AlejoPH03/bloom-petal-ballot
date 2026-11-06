//! TEMPORARY Step 1 ("hello signature") support. It is deleted after Step 1;
//! keep `snapshot` and its tests.
//!
//! One `snapshot.vote` signature over a fixed test vote proves the pipeline:
//! installer provenance, the owner's automatic policy-update ceremony, the
//! exact passkey ceremony, and an identical retry that returns a signature.
//! Nothing is submitted anywhere. Signature bytes are recovered against the
//! account address, fingerprinted with sha256 and dropped. They never reach
//! the store, a read, an error or a log.
//!
//! SDK calls (pinned rev ba76944, `crates/petal-sdk/src`):
//! - `petal::petal_use_claim` (claim.rs:581), `PetalClaim::sign_request` (claim.rs:634)
//! - `petal::ClaimEffects::moves_nothing` (claim.rs:216), `petal::ClaimNonce` (claim.rs:435)
//! - `petal::CryptoSuite::Secp256k1Keccak256Recoverable` (claim.rs:73)
//! - `petal::PayloadSignItem` (lib.rs:135), `petal::SignSelector::Exact` (lib.rs:87)
//! - `petal::sdk::sign_payload` → `petal::SignOutcome` (lib.rs:443, 226)
//! - `petal::sdk::{store_get, store_put, try_now_ms}` (lib.rs:579, 592, 650)
//! - `petal::WalletAccount::address_evm` (lib.rs:1537)

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Network;
use crate::snapshot::{self, Vote};

/// The Petal sign intent and Broker operation class.
pub const SIGN_INTENT: &str = "snapshot.vote";
pub const RECORD_SCHEMA: &str = "ballot.hello-sign.v1";
pub const MAX_BODY_BYTES: usize = 64;
const MAX_RECORD_BYTES: usize = 64 * 1024;
/// Bloom tool session used by the `verify` commands.
const VERIFY_SESSION: &str = "ballot-hello";

// The fixed test vote. Every field is a constant, so every retry signs the
// same bytes. Only `from` varies, and it is the account's own address.
pub const HELLO_SPACE: &str = "bloom-ballot-hello.eth";
pub const HELLO_PROPOSAL: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000001";
pub const HELLO_CHOICE: u32 = 1;
pub const HELLO_REASON: &str = "hello from ballot";
pub const HELLO_APP: &str = "bloom-ballot";
pub const HELLO_METADATA: &str = "{}";
/// 2026-11-06T00:00:00Z, the golden-vector constant. Never submitted, so the
/// sequencer's timestamp window does not apply.
pub const HELLO_TIMESTAMP: u64 = 1_793_923_200;

/// The fixed hello vote for a checksummed `from` address.
pub fn hello_vote(from: &str) -> Vote {
    Vote {
        from: from.to_owned(),
        space: HELLO_SPACE.into(),
        timestamp: HELLO_TIMESTAMP,
        proposal: HELLO_PROPOSAL.into(),
        choice: HELLO_CHOICE,
        reason: HELLO_REASON.into(),
        app: HELLO_APP.into(),
        metadata: HELLO_METADATA.into(),
    }
}

/// The write body: empty, or `{"attempt": n}` with n in 0..=255. The same n is
/// an identical retry. A new n is a new claim nonce, so it gets a new Bloom
/// request id for the same payload (the escape hatch for an expired approval).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt(u8);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttemptBody {
    #[serde(default)]
    attempt: u8,
}

impl Attempt {
    pub fn parse(body: &[u8]) -> Result<Self, &'static str> {
        if body.len() > MAX_BODY_BYTES {
            return Err("write body must be at most 64 bytes");
        }
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Self(0));
        }
        const SHAPE: &str = r#"write body must be empty or {"attempt": n} with n in 0..=255"#;
        // serde would also accept a JSON array for a struct; require an object.
        let value: Value = serde_json::from_slice(body).map_err(|_| SHAPE)?;
        if !value.is_object() {
            return Err(SHAPE);
        }
        serde_json::from_value::<AttemptBody>(value)
            .map(|parsed| Self(parsed.attempt))
            .map_err(|_| SHAPE)
    }

    pub fn number(self) -> u8 {
        self.0
    }

    /// Attempt 0 uses the derived nonce, so the request is reproducible from
    /// the payload alone. Attempt n > 0 uses an explicit nonce of n repeated.
    pub fn nonce(self) -> petal::ClaimNonce {
        match self.0 {
            0 => petal::ClaimNonce::Derived,
            n => petal::ClaimNonce::Explicit([n; 16]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NotAttempted,
    AwaitingApproval,
    Signed,
    SignatureMismatch,
    Error,
}

/// What a pending signing request is waiting on. Machine returns either the
/// wallet policy-update operation id (when the package is not yet allowed) or
/// the exact request id. Both are 64 lowercase hex, so the route cannot tell
/// them apart and names both owner locations. Exactly one of them exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub action_id: String,
    pub expires_ms: u64,
    pub owner_finds_ceremony_at: Vec<String>,
}

impl Pending {
    pub fn new(wallet: &str, action_id: &str, expires_ms: u64) -> Self {
        Self {
            action_id: action_id.to_owned(),
            expires_ms,
            owner_finds_ceremony_at: vec![
                format!("/petal-signing-requests/{action_id}.json"),
                format!("/wallets/{wallet}/policy-updates/pending/{action_id}/approval_challenge.json"),
            ],
        }
    }
}

/// The persisted attempt record (store, `state` namespace). By construction it
/// has no field that can hold signature bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: String,
    pub status: Status,
    pub attempt: u8,
    pub claimed_hash: String,
    pub signer_address: String,
    pub pending: Option<Pending>,
    pub signature_recovers_to_account: Option<bool>,
    pub signature_sha256: Option<String>,
    pub error: Option<String>,
    pub first_attempt_ms: u64,
    pub updated_ms: u64,
}

impl Record {
    pub fn new(attempt: Attempt, claimed_hash: &str, signer_address: &str, now_ms: u64) -> Self {
        Self {
            schema: RECORD_SCHEMA.into(),
            status: Status::NotAttempted,
            attempt: attempt.number(),
            claimed_hash: claimed_hash.to_owned(),
            signer_address: signer_address.to_owned(),
            pending: None,
            signature_recovers_to_account: None,
            signature_sha256: None,
            error: None,
            first_attempt_ms: now_ms,
            updated_ms: now_ms,
        }
    }

    fn reset_outcome(&mut self, now_ms: u64) {
        self.pending = None;
        self.signature_recovers_to_account = None;
        self.signature_sha256 = None;
        self.error = None;
        self.updated_ms = now_ms;
    }

    pub fn awaiting(&mut self, pending: Pending, now_ms: u64) {
        self.reset_outcome(now_ms);
        self.status = Status::AwaitingApproval;
        self.pending = Some(pending);
    }

    /// Record a returned signature by its recovery result and fingerprint only.
    pub fn signed(&mut self, recovers_to_account: bool, fingerprint: String, now_ms: u64) {
        self.reset_outcome(now_ms);
        self.status = if recovers_to_account {
            Status::Signed
        } else {
            Status::SignatureMismatch
        };
        self.signature_recovers_to_account = Some(recovers_to_account);
        self.signature_sha256 = Some(fingerprint);
    }

    pub fn failed(&mut self, message: &str, now_ms: u64) {
        self.reset_outcome(now_ms);
        self.status = Status::Error;
        self.error = Some(message.to_owned());
    }
}

/// Store key for one network and account. `wallet` is the host-resolved,
/// single-segment wallet id from `petal::wallet_account`.
pub fn record_key(network: Network, wallet: &str, account: u32) -> String {
    format!("hello/{}/{wallet}/{account}.json", network.as_str())
}

/// The VFS path of this route for a given account (used in `verify`).
pub fn route_path(network: Network, wallet: &str, account: u32) -> String {
    format!(
        "/petals/ballot/{}/accounts/{wallet}/{account}/hello-sign.json",
        network.as_str()
    )
}

/// Exact commands for the owner to recompute the digest with Bloom's own
/// EIP-712 tool (`bloom-vfs/src/handlers/tools.rs:17`, `539-545`), without
/// trusting Ballot's hashing.
pub fn verify_section(route: &str, claimed_hash: &str) -> Value {
    json!({
        "why": "The ceremony shows only the digest. Recompute it from the readable typed_data with Bloom's own tool and compare.",
        "commands": [
            format!("bloom vfs cat {route} | jq -c .typed_data | bloom vfs write /tools/eip712/hash/{VERIFY_SESSION}/in.json"),
            format!("bloom vfs cat /tools/eip712/hash/{VERIFY_SESSION}/out.hex"),
        ],
        "expected_out_hex": claimed_hash,
        "compare_with": "the exact hash in the ceremony's \"The exact plan that was reviewed\" (exact_ordered_hashes; shown there without 0x)",
    })
}

/// The read document: the record (or a not-attempted placeholder), plus the
/// public typed data, its preimage and the verify commands.
pub fn read_document(
    network: Network,
    wallet: &str,
    account: u32,
    vote: &Vote,
    record: Option<&Record>,
) -> Value {
    let claimed_hash = snapshot::hex32(&vote.digest());
    let route = route_path(network, wallet, account);
    let mut document = match record {
        Some(record) => serde_json::to_value(record).unwrap_or_else(|_| json!({})),
        None => json!({"schema": RECORD_SCHEMA, "status": Status::NotAttempted}),
    };
    if let Value::Object(map) = &mut document {
        map.insert("temporary".into(), json!("Step 1 pipeline proof; this route is removed after Step 1. Nothing is submitted to Snapshot."));
        map.insert("network".into(), json!(network.as_str()));
        map.insert("wallet".into(), json!(wallet));
        map.insert("account".into(), json!(account));
        map.insert("sign_intent".into(), json!(SIGN_INTENT));
        map.insert("claimed_hash".into(), json!(claimed_hash));
        map.insert("preimage".into(), json!(format!("0x{}", alloy_primitives::hex::encode(vote.preimage()))));
        map.insert("typed_data".into(), vote.typed_data());
        map.insert("verify".into(), verify_section(&route, &claimed_hash));
        map.insert(
            "write".into(),
            json!(format!(r#"bloom vfs write {route} --data '{{"attempt":0}}' (retry with the same body; a new attempt number only if an approval expired)"#)),
        );
    }
    document
}

// ---------------------------------------------------------------------------
// Host-backed operations (run only inside the route component)
// ---------------------------------------------------------------------------

fn sdk_message(error: &petal::SdkError) -> String {
    // SdkError::message() is a fixed status word or the host's own text. It
    // never contains signature bytes: none exist when signing fails.
    error.message()
}

/// The account's EVM address from Bloom's projection, EIP-55 checksummed.
pub fn account_address(account: &petal::WalletAccount) -> Result<String, petal::DispatchResponse> {
    let raw = account.address_evm()?;
    snapshot::checksum_address(&raw)
        .map_err(|reason| petal::error(-4, format!("account address.evm is invalid: {reason}")))
}

fn load_record(key: &str) -> Result<Option<Record>, petal::DispatchResponse> {
    match petal::sdk::store_get(key, MAX_RECORD_BYTES) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| petal::error(-4, "stored hello record is unreadable")),
        Err(petal::SdkError::Host(petal::HostStatus::NotFound)) => Ok(None),
        Err(error) => Err(petal::error(-4, format!("read hello record: {}", sdk_message(&error)))),
    }
}

fn save_record(key: &str, record: &Record) -> Result<(), petal::DispatchResponse> {
    let bytes = serde_json::to_vec(record)
        .map_err(|_| petal::error(-4, "encode hello record"))?;
    // secret = false: the state namespace. The record holds no secret.
    petal::sdk::store_put(key, &bytes, false)
        .map_err(|error| petal::error(-4, format!("write hello record: {}", sdk_message(&error))))
}

fn now_ms() -> u64 {
    petal::sdk::try_now_ms().unwrap_or(0)
}

/// Read handler body.
pub fn read_hello(network: Network, account: &petal::WalletAccount) -> petal::DispatchResponse {
    let from = match account_address(account) {
        Ok(from) => from,
        Err(response) => return response,
    };
    let vote = hello_vote(&from);
    let record = match load_record(&record_key(network, account.wallet(), account.number())) {
        Ok(record) => record,
        Err(response) => return response,
    };
    petal::read_json_value(&read_document(
        network,
        account.wallet(),
        account.number(),
        &vote,
        record.as_ref(),
    ))
}

/// Write handler body: request one exact `snapshot.vote` signature over the
/// fixed hello vote and record the outcome.
pub fn sign_hello(
    ctx: &petal::Ctx,
    network: Network,
    account: &petal::WalletAccount,
    attempt: Attempt,
) -> petal::DispatchResponse {
    let from = match account_address(account) {
        Ok(from) => from,
        Err(response) => return response,
    };
    let vote = hello_vote(&from);
    let digest = vote.digest();
    let claimed_hash = snapshot::hex32(&digest);
    let key = record_key(network, account.wallet(), account.number());
    let now = now_ms();
    let mut record = match load_record(&key) {
        Ok(Some(existing)) if existing.attempt == attempt.number() && existing.claimed_hash == claimed_hash => existing,
        Ok(_) => Record::new(attempt, &claimed_hash, &from, now),
        Err(response) => return response,
    };

    let payloads = [petal::PayloadSignItem {
        preimage: vote.preimage().to_vec(),
        claimed_hash: digest.0,
    }];
    let claim = match petal::petal_use_claim(
        ctx,
        SIGN_INTENT,
        petal::CryptoSuite::Secp256k1Keccak256Recoverable,
        &payloads,
        &petal::ClaimEffects::moves_nothing(),
        attempt.nonce(),
    ) {
        Ok(claim) => claim,
        Err(response) => return response,
    };
    let mut request = match claim.sign_request(account.wallet(), petal::SignSelector::Exact) {
        Ok(request) => request,
        Err(response) => return response,
    };
    // Bind the readable vote to the approval (Machine records its sha256 in
    // the exact facts). Deterministic for a given package, so retries match.
    // approval_hint, advisory and key_ref_jcs stay None: never pass a hint.
    request.action = serde_json::to_vec(&vote.typed_data()).ok();

    let outcome = petal::sdk::sign_payload(&request);
    let now = now_ms();
    match outcome {
        Ok(petal::SignOutcome::Signature(signature)) => {
            let recovers = snapshot::recover_signer(&digest, &signature)
                .ok()
                .is_some_and(|recovered| recovered.to_checksum(None) == from);
            let fingerprint = snapshot::signature_fingerprint(&signature);
            drop(signature);
            record.signed(recovers, fingerprint, now);
            if let Err(response) = save_record(&key, &record) {
                return response;
            }
            if recovers {
                petal::DispatchResponse::Write
            } else {
                petal::error(-4, "Bloom returned a signature that does not recover to this account")
            }
        }
        Ok(petal::SignOutcome::ApprovalPending { action_id, expires_ms }) => {
            let pending = Pending::new(account.wallet(), &action_id, expires_ms);
            record.awaiting(pending, now);
            if let Err(response) = save_record(&key, &record) {
                return response;
            }
            // -3, not -2: Bloom drops the message of a denial.
            petal::error(
                -3,
                format!(
                    "approval pending ({action_id}); the owner opens the ceremony from /petal-signing-requests/{action_id}.json or /wallets/{}/policy-updates/pending/{action_id}/approval_challenge.json, then retry the exact same write. Read this file for details.",
                    account.wallet()
                ),
            )
        }
        Err(error) => {
            let message = match &error {
                petal::SdkError::Host(petal::HostStatus::Denied) => {
                    "signing denied: missing installer provenance, a policy that does not allow this package, an approval that is not active, spent or expired (retry with a new attempt number), or a live ceremony on this wallet. See Machine's petal.sign_payload_denied log line.".to_owned()
                }
                other => format!("signing failed: {}", sdk_message(other)),
            };
            record.failed(&message, now);
            if let Err(response) = save_record(&key, &record) {
                return response;
            }
            let code = if matches!(error, petal::SdkError::Host(petal::HostStatus::Denied)) { -3 } else { -4 };
            petal::error(code, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: &str = "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596";

    #[test]
    fn attempt_parses_empty_and_small_bodies() {
        assert_eq!(Attempt::parse(b"").unwrap().number(), 0);
        assert_eq!(Attempt::parse(b"  \n").unwrap().number(), 0);
        assert_eq!(Attempt::parse(b"{}").unwrap().number(), 0);
        assert_eq!(Attempt::parse(br#"{"attempt":0}"#).unwrap().number(), 0);
        assert_eq!(Attempt::parse(br#"{"attempt": 7}"#).unwrap().number(), 7);
        assert_eq!(Attempt::parse(br#"{"attempt":255}"#).unwrap().number(), 255);
    }

    #[test]
    fn attempt_rejects_bad_bodies() {
        for bad in [
            &br#"{"attempt":256}"#[..],
            br#"{"attempt":-1}"#,
            br#"{"attempt":"1"}"#,
            br#"{"attempt":1,"x":2}"#,
            b"[]",
            b"hello",
            &[b' '; 65],
        ] {
            assert!(Attempt::parse(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn attempt_zero_derives_the_nonce_and_others_are_explicit() {
        assert_eq!(Attempt::parse(b"").unwrap().nonce(), petal::ClaimNonce::Derived);
        assert_eq!(
            Attempt::parse(br#"{"attempt":3}"#).unwrap().nonce(),
            petal::ClaimNonce::Explicit([3; 16])
        );
    }

    #[test]
    fn hello_vote_is_fixed_except_for_from() {
        let a = hello_vote(FROM);
        let b = hello_vote(FROM);
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest(), "identical retries sign identical bytes");
        assert_eq!(a.timestamp, HELLO_TIMESTAMP);
        assert_eq!(a.typed_data()["message"]["from"], FROM);
        assert!(crate::validate_space(HELLO_SPACE).is_ok());
        assert!(crate::validate_proposal_id(HELLO_PROPOSAL).is_ok());
        assert_ne!(hello_vote(&FROM.to_ascii_lowercase()).digest(), a.digest());
    }

    #[test]
    fn record_never_holds_signature_bytes() {
        let attempt = Attempt::parse(b"").unwrap();
        let mut record = Record::new(attempt, "0xabc", FROM, 1);
        record.signed(true, snapshot::signature_fingerprint(&[7_u8; 65]), 2);
        let text = serde_json::to_string(&record).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        for key in keys {
            assert!(
                key == "signature_sha256" || key == "signature_recovers_to_account" || !key.contains("signature"),
                "unexpected signature-like field {key}"
            );
        }
        // No 130-hex-digit run (a 65-byte signature) anywhere in the record.
        let longest_hex_run = text
            .split(|c: char| !c.is_ascii_hexdigit())
            .map(str::len)
            .max()
            .unwrap_or(0);
        assert!(longest_hex_run < 130);
        assert_eq!(record.status, Status::Signed);
        assert_eq!(record.signature_recovers_to_account, Some(true));
    }

    #[test]
    fn record_transitions_clear_previous_outcomes() {
        let attempt = Attempt::parse(b"").unwrap();
        let mut record = Record::new(attempt, "0xabc", FROM, 1);
        assert_eq!(record.status, Status::NotAttempted);
        record.awaiting(Pending::new("main", &"a".repeat(64), 99), 2);
        assert_eq!(record.status, Status::AwaitingApproval);
        assert!(record.pending.is_some());
        record.signed(false, "f".repeat(64), 3);
        assert_eq!(record.status, Status::SignatureMismatch);
        assert!(record.pending.is_none());
        record.failed("x", 4);
        assert_eq!(record.status, Status::Error);
        assert!(record.signature_sha256.is_none());
        assert_eq!(record.first_attempt_ms, 1);
        assert_eq!(record.updated_ms, 4);
    }

    #[test]
    fn pending_names_both_owner_locations() {
        let id = "b".repeat(64);
        let pending = Pending::new("main", &id, 5);
        assert_eq!(
            pending.owner_finds_ceremony_at,
            vec![
                format!("/petal-signing-requests/{id}.json"),
                format!("/wallets/main/policy-updates/pending/{id}/approval_challenge.json"),
            ]
        );
    }

    #[test]
    fn read_document_exposes_typed_data_and_bloom_verify_commands() {
        let vote = hello_vote(FROM);
        let doc = read_document(Network::Testnet, "main", 0, &vote, None);
        assert_eq!(doc["status"], "not_attempted");
        assert_eq!(doc["typed_data"], vote.typed_data());
        assert_eq!(doc["claimed_hash"], snapshot::hex32(&vote.digest()));
        assert_eq!(doc["verify"]["expected_out_hex"], doc["claimed_hash"]);
        let commands = doc["verify"]["commands"].as_array().unwrap();
        assert_eq!(
            commands[0],
            "bloom vfs cat /petals/ballot/testnet/accounts/main/0/hello-sign.json | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-hello/in.json"
        );
        assert_eq!(commands[1], "bloom vfs cat /tools/eip712/hash/ballot-hello/out.hex");
        let preimage = doc["preimage"].as_str().unwrap();
        assert!(preimage.starts_with("0x1901") && preimage.len() == 2 + 2 * 66);
    }

    #[test]
    fn record_key_and_route_path_are_account_scoped() {
        assert_eq!(record_key(Network::Mainnet, "main", 3), "hello/mainnet/main/3.json");
        assert_eq!(
            route_path(Network::Testnet, "main", 0),
            "/petals/ballot/testnet/accounts/main/0/hello-sign.json"
        );
    }
}
