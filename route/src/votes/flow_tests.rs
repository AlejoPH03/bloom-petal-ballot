//! Host tests of the vote state machine (docs/plan-votes.md §12) against a
//! scripted fake host. Signatures are made at test time by a fixed test key
//! and are never printed: assertions on anything that could contain them
//! compare without formatting the values.

use std::collections::VecDeque;

use alloy_primitives::{Address, keccak256};
use serde_json::{Value, json};

use super::*;
use crate::proposals::HubProposal;
use crate::votes::store::fake::MemStore;
use crate::votes::store::{DelIfValue, KvStore, LEASE_STALE_MS, PutNew};

const P: &str = "0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00";
const CLOSED: &str = "0xb2a3f2fba4445f4672c2b2151ba32675ca5e3144f22910205ea1b159300425f2";
const SPACE: &str = "bloom-ballot-182c.eth";
const START: u64 = 1_793_988_890;
const END: u64 = 1_794_593_689;
/// Inside the voting window.
const NOW_MS: u64 = 1_794_000_000_000;
/// An end within the sequencer's 6-day timestamp window of NOW_MS (a 7-day
/// proposal like END hits `timestamp_stale` before its end).
const NEAR_END: u64 = NOW_MS / 1000 + 3 * 86_400;
const EXACT_ID: &str = "e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1";
const POLICY_ID: &str = "a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0";

#[derive(Clone, Debug)]
enum SignScript {
    Pending(&'static str, u64),
    Sign,
    /// A valid signature by another key.
    WrongKey,
    Denied,
    Failed,
}

#[derive(Clone, Debug)]
enum PostScript {
    /// The sequencer stores the vote and answers 200.
    Accept,
    /// The sequencer stores the vote, the response is lost.
    AcceptLost,
    /// An error response; the vote is not stored.
    Error(u16, &'static str),
    /// No response; the vote is not stored.
    Transport,
}

struct FakeHost {
    store: MemStore,
    now: Option<u64>,
    proposal: Result<Option<HubProposal>, HubError>,
    vp: Result<f64, HubError>,
    hub_votes: Vec<HubVote>,
    hub_down: bool,
    sign_script: VecDeque<SignScript>,
    default_sign: Option<SignScript>,
    post_script: VecDeque<PostScript>,
    calls: Vec<&'static str>,
    posted: Vec<Vec<u8>>,
}

fn key(seed: u8) -> k256::ecdsa::SigningKey {
    k256::ecdsa::SigningKey::from_bytes(&[seed; 32].into()).expect("valid key")
}

fn address_of(seed: u8) -> String {
    let point = key(seed).verifying_key().to_encoded_point(false);
    Address::from_slice(&keccak256(&point.as_bytes()[1..])[12..]).to_checksum(None)
}

fn sign_with(seed: u8, vote: &Vote) -> SignatureBytes {
    let (signature, recovery) = key(seed).sign_prehash_recoverable(vote.digest().as_slice()).unwrap();
    let mut raw = signature.to_bytes().to_vec();
    raw.push(recovery.to_byte());
    SignatureBytes::new(raw)
}

fn proposal(id: &str, state: &str, kind: &str, start: u64, end: u64) -> HubProposal {
    serde_json::from_value(json!({
        "id": id, "space": {"id": SPACE}, "author": "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596",
        "title": "bloom-ballot test", "body": "", "choices": ["For", "Against", "Abstain"], "type": kind,
        "start": start, "end": end, "snapshot": 12078262, "state": state, "scores": [1, 0, 0],
        "scores_total": 1, "votes": 1
    }))
    .unwrap()
}

impl FakeHost {
    fn new() -> Self {
        Self {
            store: MemStore::default(),
            now: Some(NOW_MS),
            proposal: Ok(Some(proposal(P, "active", "single-choice", START, END))),
            vp: Ok(1.0),
            hub_votes: Vec::new(),
            hub_down: false,
            sign_script: VecDeque::new(),
            default_sign: None,
            post_script: VecDeque::new(),
            calls: Vec::new(),
            posted: Vec::new(),
        }
    }

    fn signs(mut self, script: &[SignScript]) -> Self {
        self.sign_script.extend(script.iter().cloned());
        self
    }

    fn posts(mut self, script: &[PostScript]) -> Self {
        self.post_script.extend(script.iter().cloned());
        self
    }

    fn external_calls(&self) -> Vec<&'static str> {
        self.calls.clone()
    }

    /// Store a vote the sequencer accepted, as the hub will show it.
    fn sequencer_stores(&mut self, body: &[u8]) -> Result<(), &'static str> {
        let envelope: Value = serde_json::from_slice(body).expect("envelope is JSON");
        let m = &envelope["data"]["message"];
        let vote = Vote {
            from: m["from"].as_str().unwrap().into(),
            space: m["space"].as_str().unwrap().into(),
            timestamp: m["timestamp"].as_u64().unwrap(),
            proposal: m["proposal"].as_str().unwrap().into(),
            choice: m["choice"].as_u64().unwrap() as u32,
            reason: m["reason"].as_str().unwrap().into(),
            app: m["app"].as_str().unwrap().into(),
            metadata: m["metadata"].as_str().unwrap().into(),
        };
        assert_eq!(envelope["address"], m["from"], "envelope address is the signer");
        // The sequencer verifies the signature (ingestor.ts:108-117).
        let sig = alloy_primitives::hex::decode(envelope["sig"].as_str().unwrap()).unwrap();
        let signer = snapshot::recover_signer(&vote.digest(), &sig).expect("recoverable");
        assert!(signer.to_checksum(None) == vote.from, "signature validation failed");
        let id = snapshot::hex32(&vote.digest());
        if self.hub_votes.iter().any(|v| v.id == id) {
            return Err("duplicate message");
        }
        self.hub_votes.retain(|v| !v.voter.eq_ignore_ascii_case(&vote.from));
        self.hub_votes.push(HubVote { id, voter: vote.from, choice: json!(vote.choice), created: vote.timestamp });
        Ok(())
    }
}

impl KvStore for FakeHost {
    fn store_get(&mut self, key: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        self.store.store_get(key, max)
    }
    fn store_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
        self.store.store_put(key, value)
    }
    fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<PutNew, String> {
        self.store.store_put_new(key, value)
    }
    fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<DelIfValue, String> {
        self.store.store_del_if_value(key, expected)
    }
    fn store_list(&mut self, prefix: &str, max: usize) -> Result<Vec<String>, String> {
        self.store.store_list(prefix, max)
    }
    fn secret_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
        self.store.secret_put(key, value)
    }
    fn secret_get(&mut self, key: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        self.store.secret_get(key, max)
    }
    fn secret_del(&mut self, key: &str) -> Result<(), String> {
        self.store.secret_del(key)
    }
}

impl VoteHost for FakeHost {
    fn now_ms(&mut self) -> Option<u64> {
        self.now
    }
    fn random16(&mut self) -> Option<[u8; 16]> {
        Some([7; 16])
    }
    fn account_address(&mut self) -> Result<String, String> {
        self.calls.push("vfs.address");
        Ok(address_of(0x11))
    }
    fn hub_proposal(&mut self, _network: Network, id: &str) -> Result<Option<HubProposal>, HubError> {
        self.calls.push("hub.proposal");
        self.proposal.clone().map(|p| p.filter(|p| p.id == id))
    }
    fn hub_vp(&mut self, _network: Network, _voter: &str, _space: &str, _proposal: &str) -> Result<f64, HubError> {
        self.calls.push("hub.vp");
        self.vp.clone()
    }
    fn hub_votes(&mut self, _network: Network, proposal: &str, voter: &str) -> Result<Vec<HubVote>, HubError> {
        self.calls.push("hub.votes");
        if self.hub_down {
            return Err(HubError::Status(502));
        }
        let _ = proposal;
        Ok(self.hub_votes.iter().filter(|v| v.voter.eq_ignore_ascii_case(voter)).cloned().collect())
    }
    fn sign(&mut self, vote: &Vote) -> SignResult {
        self.calls.push("sign");
        let script = self.sign_script.pop_front().or_else(|| self.default_sign.clone()).expect("unscripted sign call");
        match script {
            SignScript::Pending(id, expires) => SignResult::Pending { action_id: id.into(), expires_ms: expires },
            SignScript::Sign => SignResult::Signature(sign_with(0x11, vote)),
            SignScript::WrongKey => SignResult::Signature(sign_with(0x22, vote)),
            SignScript::Denied => SignResult::Denied,
            SignScript::Failed => SignResult::Failed("backend: broker unavailable".into()),
        }
    }
    fn post_vote(&mut self, _network: Network, body: &[u8]) -> PostResult {
        self.calls.push("sequencer.post");
        self.posted.push(body.to_vec());
        let script = self.post_script.pop_front().expect("unscripted post");
        match script {
            PostScript::Accept => match self.sequencer_stores(body) {
                Ok(()) => {
                    let id = self.hub_votes.last().unwrap().id.clone();
                    let body = json!({"id": id, "ipfs": "bafkreitest", "relayer": {"address": "0x8BBE4Ac64246d600BC2889ef5d83809D138F03DF", "receipt": "0xabcdef"}});
                    PostResult::Http { status: 200, body: body.to_string().into_bytes() }
                }
                Err(text) => error_response(400, text),
            },
            PostScript::AcceptLost => {
                let _ = self.sequencer_stores(body);
                PostResult::Transport("connection reset".into())
            }
            PostScript::Error(status, text) => error_response(status, text),
            PostScript::Transport => PostResult::Transport("timed out".into()),
        }
    }
}

fn error_response(status: u16, text: &str) -> PostResult {
    let body = json!({"error": if status < 500 { "client_error" } else { "server_error" }, "error_description": text});
    PostResult::Http { status, body: body.to_string().into_bytes() }
}

fn scope() -> Scope {
    Scope::new(Network::Testnet, "main", 0)
}

fn body(id: &str, proposal: &str, choice: u32, reason: Option<&str>) -> Vec<u8> {
    let mut value = json!({"vote_id": id, "space": SPACE, "proposal": proposal, "choice": choice});
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    serde_json::to_vec(&value).unwrap()
}

fn demo(id: &str) -> Vec<u8> {
    body(id, P, 1, Some("ballot test"))
}

fn record(host: &mut FakeHost, id: &str) -> Option<Record> {
    get_json(host, &scope().record_key(id), "record").unwrap()
}

fn binding(host: &mut FakeHost, id: &str) -> Option<Binding> {
    get_json(host, &scope().binding_key(id), "binding").unwrap()
}

fn marker(host: &mut FakeHost) -> Marker {
    get_json(host, &scope().marker_key(), "marker").unwrap().expect("marker written")
}

fn send(host: &mut FakeHost, bytes: &[u8]) -> WriteOutcome {
    host.calls.clear();
    run_write(host, &scope(), bytes)
}

fn has_secret(host: &FakeHost, id: &str) -> bool {
    host.store.secrets.contains_key(&scope().signature_key(id))
}

/// No signature bytes outside the secrets namespace: every state value and
/// every read document is free of each signature produced in the test and of
/// any 130+ hex run other than the public preimage.
fn assert_secret_wall(host: &mut FakeHost, ids: &[&str]) {
    let mut signatures: Vec<String> = host.store.secrets.values().map(alloy_primitives::hex::encode).collect();
    for body in &host.posted {
        let envelope: Value = serde_json::from_slice(body).unwrap();
        signatures.push(envelope["sig"].as_str().unwrap().trim_start_matches("0x").to_owned());
    }
    let mut texts: Vec<String> = host.store.state.values().map(|v| String::from_utf8_lossy(v).into_owned()).collect();
    for id in ids {
        if let Some(doc) = vote_document(host, &scope(), id).unwrap() {
            texts.push(doc.to_string());
        }
    }
    texts.push(new_document(host, &scope()).unwrap().to_string());
    for text in &texts {
        let lower = text.to_ascii_lowercase();
        for sig in &signatures {
            assert!(!lower.contains(sig.as_str()), "signature bytes leaked into readable state");
        }
        for run in text.split(|c: char| !c.is_ascii_hexdigit()) {
            assert!(run.len() < 130 || (run.len() == 132 && run.starts_with("1901")), "a 130+ hex run that is not the preimage");
        }
    }
    for key in host.store.secrets.keys() {
        assert!(key.starts_with("creds/votes/"), "secret outside creds/votes/: {key}");
    }
}

// ---------------------------------------------------------------------------
// Happy path and identical retries
// ---------------------------------------------------------------------------

#[test]
fn policy_ceremony_then_signing_ceremony_then_submitted_then_counted() {
    let mut host = FakeHost::new()
        .signs(&[SignScript::Pending(POLICY_ID, NOW_MS + 300_000), SignScript::Pending(EXACT_ID, NOW_MS + 300_000), SignScript::Pending(EXACT_ID, NOW_MS + 300_000), SignScript::Sign])
        .posts(&[PostScript::Accept]);

    let first = send(&mut host, &demo("demo-1"));
    assert_eq!((first.code, first.outcome), (-3, "approval_pending"));
    assert!(first.message.starts_with(&format!("approval pending ({POLICY_ID})")), "{}", first.message);
    assert_eq!(host.external_calls(), ["hub.proposal", "vfs.address", "hub.vp", "sign"]);
    let bound = binding(&mut host, "demo-1").expect("bound");
    assert_eq!(bound.timestamp, NOW_MS / 1000, "timestamp frozen at draft");
    assert_eq!(bound.from, address_of(0x11));
    assert_eq!(bound.proposal_facts.choice_label, "For");
    assert!(bound.is_consistent());
    let r = record(&mut host, "demo-1").unwrap();
    assert_eq!(r.status, Status::AwaitingApproval);
    assert_eq!(r.pending.as_ref().unwrap().action_id, POLICY_ID);
    assert_eq!(r.history.iter().map(|h| h.status).collect::<Vec<_>>(), [Status::Drafted, Status::AwaitingApproval]);
    let m = marker(&mut host);
    assert_eq!((m.code, m.outcome.as_str(), m.record.as_deref()), (-3, "approval_pending", Some("votes/demo-1.json")));
    assert_eq!(m.message, first.message, "the marker carries the exact text the write returned");

    // The owner approved the policy; the identical write reaches the exact ceremony.
    host.now = Some(NOW_MS + 60_000);
    let second = send(&mut host, &demo("demo-1"));
    assert!(second.message.starts_with(&format!("approval pending ({EXACT_ID})")));
    assert_eq!(host.external_calls(), ["sign"], "a bound vote re-checks nothing on the hub");
    assert_eq!(binding(&mut host, "demo-1").unwrap(), bound, "the binding never changes");

    // Early retry: the same pending id comes back, nothing new.
    let early = send(&mut host, &demo("demo-1"));
    assert_eq!((early.code, early.outcome), (-3, "approval_pending"));
    assert_eq!(record(&mut host, "demo-1").unwrap().pending.unwrap().action_id, EXACT_ID);

    // Approved: the identical write signs, keeps the signature secret, posts once.
    let third = send(&mut host, &demo("demo-1"));
    assert_eq!((third.code, third.outcome, third.message.as_str()), (0, "submitted", ""));
    assert_eq!(host.external_calls(), ["sign", "sequencer.post"]);
    let r = record(&mut host, "demo-1").unwrap();
    assert_eq!(r.status, Status::Submitted);
    let receipt = r.receipt.as_ref().unwrap();
    assert_eq!(receipt.id, bound.digest, "Snapshot's vote id is the digest");
    assert!(!receipt.reconciled && receipt.warning.is_none());
    assert_eq!(receipt.receipt_sha256.as_deref(), Some(sha256_hex(b"0xabcdef").as_str()));
    assert_eq!(r.submit_attempts, 1);
    assert_eq!(r.signature_sha256.as_ref().map(String::len), Some(64));
    assert!(!has_secret(&host, "demo-1"), "the secret is deleted once submitted");
    assert_eq!(host.posted.len(), 1);

    // The identical write records counted from the hub; no signing, no POST.
    let fourth = send(&mut host, &demo("demo-1"));
    assert_eq!((fourth.code, fourth.outcome), (0, "counted"));
    assert_eq!(host.external_calls(), ["hub.votes"]);
    let r = record(&mut host, "demo-1").unwrap();
    assert_eq!(r.status, Status::Counted);
    assert_eq!(r.counted.as_ref().unwrap().hub_vote_id, bound.digest);

    let fifth = send(&mut host, &demo("demo-1"));
    assert_eq!((fifth.code, fifth.outcome), (0, "counted"));
    assert!(host.external_calls().is_empty(), "counted makes no external call");
    assert_secret_wall(&mut host, &["demo-1"]);
}

#[test]
fn identical_means_normalised_fields() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    host.default_sign = Some(SignScript::Pending(EXACT_ID, NOW_MS + 300_000));
    send(&mut host, &body("v", P, 2, None));
    let reformatted = format!("{{ \"reason\": \"\", \"choice\": 2,\n \"proposal\": \"{P}\", \"space\": \"{SPACE}\", \"vote_id\": \"v\" }}\n");
    let again = send(&mut host, reformatted.as_bytes());
    assert_eq!(again.outcome, "approval_pending", "same vote: resumes");
    assert_ne!(marker(&mut host).body_sha256, sha256_hex(&body("v", P, 2, None)), "raw hash differs");
    assert_eq!(marker(&mut host).canonical_body_sha256.unwrap(), binding(&mut host, "v").unwrap().canonical_body_sha256);
}

// ---------------------------------------------------------------------------
// Refusals (unbound)
// ---------------------------------------------------------------------------

fn refused(host: &mut FakeHost, bytes: &[u8], code: &str) -> Record {
    let out = send(host, bytes);
    assert_eq!((out.code, out.outcome), (-3, "refused"), "{}", out.message);
    assert!(out.message.starts_with(&format!("refused: {code}: ")), "{}", out.message);
    let id = serde_json::from_slice::<Value>(bytes).unwrap()["vote_id"].as_str().unwrap().to_owned();
    assert!(binding(host, &id).is_none(), "a refused vote_id is not bound");
    assert!(!host.calls.contains(&"sign"), "refused before signing");
    let r = record(host, &id).unwrap();
    assert_eq!(r.status, Status::Refused);
    assert_eq!(r.reason_code.as_deref(), Some(code));
    r
}

#[test]
fn every_pre_signing_check_refuses_with_its_code() {
    let mut host = FakeHost::new();
    host.proposal = Ok(Some(proposal(CLOSED, "closed", "single-choice", 1_793_989_315, 1_793_989_914)));
    refused(&mut host, &body("closed-1", CLOSED, 1, None), "proposal_not_active");

    let mut host = FakeHost::new();
    host.proposal = Ok(None);
    refused(&mut host, &demo("a"), "proposal_not_found");

    let mut host = FakeHost::new();
    let other_space = body("a", P, 1, None);
    let mut value: Value = serde_json::from_slice(&other_space).unwrap();
    value["space"] = json!("someone-else.eth");
    let r = refused(&mut host, &serde_json::to_vec(&value).unwrap(), "space_mismatch");
    assert_eq!(r.refused_body.unwrap().space, "someone-else.eth");

    let mut host = FakeHost::new();
    host.now = Some(END * 1000);
    refused(&mut host, &demo("a"), "voting_closed");

    let mut host = FakeHost::new();
    host.proposal = Ok(Some(proposal(P, "active", "single-choice", NOW_MS / 1000 + 10, END)));
    refused(&mut host, &demo("a"), "voting_not_started");

    let mut host = FakeHost::new();
    host.proposal = Ok(Some(proposal(P, "active", "approval", START, END)));
    refused(&mut host, &demo("a"), "unsupported_vote_type");

    let mut host = FakeHost::new();
    let r = refused(&mut host, &body("bad-choice", P, 4, None), "choice_out_of_range");
    assert_eq!(r.detail.as_deref(), Some("choice 4 but this proposal has 3 choices (1-3)"));
    assert_eq!(host.external_calls(), ["hub.proposal"], "no vp check after a refusal");

    let mut host = FakeHost::new();
    host.vp = Ok(0.0);
    refused(&mut host, &demo("a"), "no_voting_power");
}

#[test]
fn mainnet_is_refused_before_any_external_call() {
    let mut host = FakeHost::new();
    let mainnet = Scope::new(Network::Mainnet, "main", 0);
    host.calls.clear();
    let out = run_write(&mut host, &mainnet, &demo("m"));
    assert_eq!((out.code, out.outcome), (-3, "refused"));
    assert!(out.message.starts_with("refused: mainnet_disabled: "));
    assert!(host.calls.is_empty());
    let r: Record = get_json(&mut host, &mainnet.record_key("m"), "r").unwrap().unwrap();
    assert_eq!(r.reason_code.as_deref(), Some("mainnet_disabled"));
}

#[test]
fn a_refused_vote_id_can_be_reused_with_a_corrected_body() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    refused(&mut host, &body("fix-me", P, 9, None), "choice_out_of_range");
    // Another refusal under the same id is not a conflict either.
    refused(&mut host, &body("fix-me", P, 7, Some("x")), "choice_out_of_range");
    let out = send(&mut host, &body("fix-me", P, 2, None));
    assert_eq!(out.outcome, "approval_pending");
    assert_eq!(binding(&mut host, "fix-me").unwrap().choice, 2);
    let r = record(&mut host, "fix-me").unwrap();
    assert_eq!(r.status, Status::AwaitingApproval);
    assert!(r.refused_body.is_none() && r.reason_code.is_none(), "the refusal is replaced");
}

#[test]
fn hub_failure_during_checks_is_transient_and_binds_nothing() {
    let mut host = FakeHost::new();
    host.proposal = Err(HubError::Status(502));
    let out = send(&mut host, &demo("a"));
    assert_eq!((out.code, out.outcome), (-4, "transient_error"));
    assert!(out.message.contains("snapshot testnet hub returned status 502"));
    assert!(record(&mut host, "a").is_none() && binding(&mut host, "a").is_none());
    let mut host = FakeHost::new();
    host.vp = Err(HubError::GraphQl("proposal not found".into()));
    assert_eq!(send(&mut host, &demo("a")).code, -4);
    assert!(binding(&mut host, "a").is_none());
}

// ---------------------------------------------------------------------------
// Bound ids: conflicts and terminal states
// ---------------------------------------------------------------------------

#[test]
fn a_different_body_under_a_bound_id_is_refused_without_external_calls() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    send(&mut host, &demo("demo-1"));
    let before = record(&mut host, "demo-1");
    for changed in [body("demo-1", P, 2, Some("ballot test")), body("demo-1", P, 1, None), body("demo-1", P, 1, Some("other"))] {
        let out = send(&mut host, &changed);
        assert_eq!((out.code, out.outcome), (-3, "conflict"));
        assert!(out.message.contains("bound to a different vote"));
        assert!(host.calls.is_empty());
        assert_eq!(record(&mut host, "demo-1"), before, "record unchanged");
    }
}

#[test]
fn final_states_answer_without_external_calls() {
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Error(400, "invalid choice")]);
    let out = send(&mut host, &demo("r"));
    assert_eq!((out.code, out.outcome), (-3, "rejected_by_snapshot"));
    assert!(out.message.starts_with("rejected by Snapshot: invalid_choice: "));
    assert!(!has_secret(&host, "r"), "secret deleted on a final state");
    let again = send(&mut host, &demo("r"));
    assert_eq!((again.code, again.outcome), (-3, "rejected_by_snapshot"));
    assert_eq!(again.message, "rejected_by_snapshot (invalid_choice): this vote_id is final; write a new vote_id to vote again");
    assert!(host.calls.is_empty());
    assert_secret_wall(&mut host, &["r"]);
}

// ---------------------------------------------------------------------------
// Signing outcomes
// ---------------------------------------------------------------------------

#[test]
fn denial_near_the_approval_deadline_is_an_inferred_expiry() {
    let expires = NOW_MS + 300_000;
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, expires), SignScript::Denied, SignScript::Denied]);
    send(&mut host, &demo("e"));
    // Well before the deadline: a refusal, not an expiry.
    host.now = Some(expires - APPROVAL_EXPIRY_MARGIN_MS - 1);
    let early = send(&mut host, &demo("e"));
    assert_eq!((early.code, early.outcome), (-3, "signing_refused"));
    assert!(early.message.contains("CEREMONY_RATE_LIMITED"));
    assert_eq!(record(&mut host, "e").unwrap().status, Status::AwaitingApproval);
    // Within the margin (Signer refuses ~90 s early): expired, inferred.
    host.now = Some(expires - APPROVAL_EXPIRY_MARGIN_MS);
    let late = send(&mut host, &demo("e"));
    assert_eq!((late.code, late.outcome), (-3, "expired"));
    assert!(late.message.contains("approval_expired") && late.message.contains("write a new vote_id"));
    let r = record(&mut host, "e").unwrap();
    assert_eq!((r.status, r.reason_code.as_deref(), r.inferred), (Status::Expired, Some("approval_expired"), true));
    assert!(r.detail.unwrap().starts_with("inferred: "));
}

#[test]
fn denial_of_a_draft_stays_drafted() {
    let mut host = FakeHost::new().signs(&[SignScript::Denied]);
    let out = send(&mut host, &demo("d"));
    assert_eq!((out.code, out.outcome), (-3, "signing_refused"));
    let r = record(&mut host, "d").unwrap();
    assert_eq!(r.status, Status::Drafted);
    assert!(r.last_error.unwrap().starts_with("signing refused"));
}

#[test]
fn signing_failure_is_transient() {
    let mut host = FakeHost::new().signs(&[SignScript::Failed]);
    let out = send(&mut host, &demo("f"));
    assert_eq!((out.code, out.outcome), (-4, "transient_error"));
    assert_eq!(record(&mut host, "f").unwrap().status, Status::Drafted);
}

#[test]
fn a_signature_by_another_key_is_dropped_and_never_posted() {
    let mut host = FakeHost::new().signs(&[SignScript::WrongKey]);
    let out = send(&mut host, &demo("w"));
    assert_eq!(out.code, -4);
    assert!(out.message.contains("does not recover"));
    assert!(!has_secret(&host, "w") && host.posted.is_empty());
    assert_eq!(record(&mut host, "w").unwrap().status, Status::Drafted);
}

// ---------------------------------------------------------------------------
// Submission: transient, lost, duplicate, permanent
// ---------------------------------------------------------------------------

#[test]
fn transient_failure_keeps_signed_and_the_retry_reposts_identical_bytes() {
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Error(503, "update in progress, try later"), PostScript::Accept]);
    let out = send(&mut host, &demo("t"));
    assert_eq!((out.code, out.outcome), (-4, "transient_error"));
    assert!(out.message.contains("update in progress"));
    let r = record(&mut host, "t").unwrap();
    assert_eq!((r.status, r.submit_attempts), (Status::Signed, 1));
    assert!(has_secret(&host, "t"));
    assert_secret_wall(&mut host, &["t"]);
    let retry = send(&mut host, &demo("t"));
    assert_eq!((retry.code, retry.outcome), (0, "submitted"));
    assert_eq!(host.external_calls(), ["hub.votes", "sequencer.post"], "hub checked before re-posting");
    assert_eq!(host.posted.len(), 2);
    assert!(host.posted[0] == host.posted[1], "the retry posts the same signed bytes");
    assert_eq!(record(&mut host, "t").unwrap().submit_attempts, 2);
}

#[test]
fn a_lost_response_is_reconciled_on_the_hub_without_a_second_post() {
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::AcceptLost]);
    let out = send(&mut host, &demo("l"));
    assert_eq!(out.code, -4);
    assert_eq!(record(&mut host, "l").unwrap().status, Status::Signed);
    let retry = send(&mut host, &demo("l"));
    assert_eq!((retry.code, retry.outcome), (0, "submitted"));
    assert_eq!(host.external_calls(), ["hub.votes"]);
    assert_eq!(host.posted.len(), 1);
    let r = record(&mut host, "l").unwrap();
    assert!(r.receipt.as_ref().unwrap().reconciled);
    assert!(!has_secret(&host, "l"));
}

#[test]
fn hub_down_after_an_attempt_never_reposts_blindly() {
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Transport]);
    send(&mut host, &demo("h"));
    host.hub_down = true;
    let retry = send(&mut host, &demo("h"));
    assert_eq!(retry.code, -4);
    assert!(retry.message.contains("not posting again"));
    assert_eq!(host.posted.len(), 1);
}

#[test]
fn duplicate_is_reconciled_or_stays_signed() {
    // The vote is already on the hub (accepted earlier, record lost the result).
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Error(400, "duplicate message")]);
    host.default_sign = None;
    // Pre-load the hub with exactly the vote this write will sign.
    let mut probe = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    send(&mut probe, &demo("dup"));
    let b = binding(&mut probe, "dup").unwrap();
    host.hub_votes.push(HubVote { id: b.digest.clone(), voter: b.from.clone(), choice: json!(1), created: b.timestamp });
    let out = send(&mut host, &demo("dup"));
    assert_eq!((out.code, out.outcome), (0, "submitted"));
    assert!(record(&mut host, "dup").unwrap().receipt.unwrap().reconciled);

    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Error(400, "duplicate message")]);
    let out = send(&mut host, &demo("dup"));
    assert_eq!(out.code, -4);
    assert!(out.message.contains("duplicate"));
    assert_eq!(record(&mut host, "dup").unwrap().status, Status::Signed);
}

#[test]
fn permanent_errors_map_to_final_states() {
    let cases: [(&str, u64, Status, &str); 5] = [
        ("no voting power", NOW_MS, Status::RejectedBySnapshot, "no_voting_power"),
        ("already voted at later time", NOW_MS, Status::RejectedBySnapshot, "newer_vote_exists"),
        ("wrong timestamp", NOW_MS, Status::RejectedBySnapshot, "clock_skew"),
        ("not in voting window", NOW_MS, Status::RejectedBySnapshot, "not_in_voting_window"),
        ("signature validation failed", NOW_MS, Status::RejectedBySnapshot, "signature_validation_failed"),
    ];
    for (text, now, status, code) in cases {
        let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Error(400, text)]);
        host.now = Some(now);
        let out = send(&mut host, &demo("p"));
        assert_eq!(out.code, -3, "{text}");
        let r = record(&mut host, "p").unwrap();
        assert_eq!((r.status, r.reason_code.as_deref()), (status, Some(code)), "{text}");
        assert!(!has_secret(&host, "p"));
    }
    // "not in voting window" after the end is an expiry: sign, then the clock
    // passes the end before the retry posts.
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Transport, PostScript::Error(400, "not in voting window")]);
    host.proposal = Ok(Some(proposal(P, "active", "single-choice", START, NEAR_END)));
    send(&mut host, &demo("late"));
    host.now = Some(NEAR_END * 1000 - 1);
    let out = send(&mut host, &demo("late"));
    assert_eq!(out.outcome, "rejected_by_snapshot", "before the end it is a rejection");
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Error(400, "not in voting window")]);
    host.proposal = Ok(Some(proposal(P, "active", "single-choice", START, NOW_MS / 1000 + 1)));
    send(&mut host, &demo("edge"));
    let r = record(&mut host, "edge").unwrap();
    assert_eq!(r.reason_code.as_deref(), Some("not_in_voting_window"));
}

// ---------------------------------------------------------------------------
// Expiry boundaries
// ---------------------------------------------------------------------------

#[test]
fn proposal_end_expires_an_unsubmitted_vote_without_signing() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000), SignScript::Pending(EXACT_ID, NEAR_END * 1000)]);
    host.proposal = Ok(Some(proposal(P, "active", "single-choice", START, NEAR_END)));
    send(&mut host, &demo("x"));
    host.now = Some(NEAR_END * 1000 - 1);
    assert_eq!(send(&mut host, &demo("x")).outcome, "approval_pending", "one ms before the end still signs");
    host.now = Some(NEAR_END * 1000);
    let out = send(&mut host, &demo("x"));
    assert_eq!((out.code, out.outcome), (-3, "expired"));
    assert!(out.message.contains("proposal_closed"));
    assert!(host.calls.is_empty(), "no signing after the end");
}

#[test]
fn a_frozen_timestamp_expires_before_the_sequencer_limit() {
    let far_end = NOW_MS / 1000 + 30 * 86_400;
    let limit = NOW_MS / 1000 + SEQUENCER_MAX_AGE_SECS - STALE_MARGIN_SECS;
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000), SignScript::Pending(EXACT_ID, u64::MAX)]);
    host.proposal = Ok(Some(proposal(P, "active", "single-choice", START, far_end)));
    send(&mut host, &demo("s"));
    host.now = Some(limit * 1000);
    assert_eq!(send(&mut host, &demo("s")).outcome, "approval_pending");
    host.now = Some((limit + 1) * 1000);
    let out = send(&mut host, &demo("s"));
    assert_eq!(out.outcome, "expired");
    assert!(out.message.contains("timestamp_stale"));
}

#[test]
fn a_signed_vote_past_the_end_is_checked_on_the_hub_first() {
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::AcceptLost]);
    send(&mut host, &demo("z"));
    host.now = Some(END * 1000 + 5);
    let out = send(&mut host, &demo("z"));
    assert_eq!(out.outcome, "submitted", "the hub shows it: not expired");
    let mut host = FakeHost::new().signs(&[SignScript::Sign]).posts(&[PostScript::Transport]);
    send(&mut host, &demo("z"));
    host.now = Some(END * 1000 + 5);
    let out = send(&mut host, &demo("z"));
    assert_eq!(out.outcome, "expired");
    assert!(!has_secret(&host, "z"));
}

// ---------------------------------------------------------------------------
// Crash safety, binding, lease, body errors
// ---------------------------------------------------------------------------

/// Fail every store mutation after the k-th one, for each k along a full
/// draft → sign → submit write; then retry until the vote settles. The vote
/// is never posted with different bytes, never twice once the hub shows it,
/// and the hub never holds two votes.
#[test]
fn crash_at_any_store_write_converges() {
    for k in 0..12 {
        let mut host = FakeHost::new();
        host.default_sign = Some(SignScript::Sign);
        host.post_script = VecDeque::from(vec![PostScript::Accept; 4]);
        host.store.writes_left = Some(k);
        send(&mut host, &demo("c"));
        host.store.writes_left = None;
        let mut status = None;
        for round in 1..=6 {
            host.now = Some(NOW_MS + round * (LEASE_STALE_MS + 1_000));
            let out = send(&mut host, &demo("c"));
            status = record(&mut host, "c").map(|r| r.status);
            if out.outcome == "counted" {
                break;
            }
        }
        assert_eq!(status, Some(Status::Counted), "k = {k}");
        assert_eq!(host.hub_votes.len(), 1, "k = {k}");
        assert!(host.posted.windows(2).all(|w| w[0] == w[1]), "k = {k}: every POST carries the same bytes");
        let posts_after_hub = host.posted.len();
        assert!(posts_after_hub <= 2, "k = {k}: {posts_after_hub} posts");
        assert!(!has_secret(&host, "c"), "k = {k}");
        assert_secret_wall(&mut host, &["c"]);
    }
}

#[test]
fn a_binding_without_a_record_resumes_as_a_draft() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000), SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    send(&mut host, &demo("b"));
    host.store.state.remove(&scope().record_key("b"));
    let out = send(&mut host, &demo("b"));
    assert_eq!(out.outcome, "approval_pending");
    assert_eq!(host.external_calls(), ["sign"]);
}

#[test]
fn a_lost_secret_is_signed_again_over_the_same_payload() {
    let mut host = FakeHost::new().signs(&[SignScript::Sign, SignScript::Sign]).posts(&[PostScript::Transport, PostScript::Accept]);
    send(&mut host, &demo("g"));
    host.store.secrets.clear();
    let out = send(&mut host, &demo("g"));
    assert_eq!(out.outcome, "submitted");
    // Hub check (attempt 1 happened), re-sign, hub check again on the way
    // to the re-post (the record went through drafted), then the POST.
    assert_eq!(host.external_calls(), ["hub.votes", "sign", "hub.votes", "sequencer.post"]);
    assert!(host.posted[0] == host.posted[1], "RFC 6979: the same payload signs to the same bytes");
}

#[test]
fn an_inconsistent_binding_is_never_signed() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    send(&mut host, &demo("i"));
    let key = scope().binding_key("i");
    let mut b: Binding = get_json(&mut host, &key, "b").unwrap().unwrap();
    b.timestamp += 1;
    put_json(&mut host, &key, &b, "b").unwrap();
    let out = send(&mut host, &demo("i"));
    assert_eq!(out.code, -4);
    assert!(!host.calls.contains(&"sign"));
}

#[test]
fn a_concurrent_write_is_busy_until_the_lease_is_stale() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    let lease_key = scope().lease_key("busy");
    let held = Lease::acquire(&mut host, &lease_key, [9; 16], NOW_MS).unwrap();
    let out = send(&mut host, &demo("busy"));
    assert_eq!((out.code, out.outcome), (-3, "busy"));
    assert!(host.calls.is_empty());
    assert_eq!(marker(&mut host).record, None);
    host.now = Some(NOW_MS + LEASE_STALE_MS + 1);
    assert_eq!(send(&mut host, &demo("busy")).outcome, "approval_pending");
    held.release(&mut host);
    assert!(!host.store.state.contains_key(&lease_key), "released after the write");
}

#[test]
fn an_invalid_body_leaves_only_the_marker() {
    let mut host = FakeHost::new();
    let bad = br#"{"vote_id":"ok-1","space":"bloom-ballot-182c.eth","proposal":"0x00","choice":1,"extra":true}"#;
    let out = send(&mut host, bad);
    assert_eq!((out.code, out.outcome), (-3, "invalid_body"));
    assert!(out.message.starts_with("invalid body: "));
    assert!(host.calls.is_empty());
    let m = marker(&mut host);
    assert_eq!((m.vote_id.as_deref(), m.record, m.body_sha256), (Some("ok-1"), None, sha256_hex(bad)));
    assert!(record(&mut host, "ok-1").is_none());
}

#[test]
fn no_clock_is_transient() {
    let mut host = FakeHost::new();
    host.now = Some(0);
    let out = send(&mut host, &demo("n"));
    assert_eq!((out.code, out.outcome), (-4, "transient_error"));
    assert!(host.calls.is_empty());
}

// ---------------------------------------------------------------------------
// Protocol facts
// ---------------------------------------------------------------------------

#[test]
fn the_digest_is_snapshots_vote_id() {
    let m: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/snapshot-testnet/golden/live_vote_message.json"
    )))
    .unwrap();
    let vote = Vote {
        from: m["from"].as_str().unwrap().into(),
        space: m["space"].as_str().unwrap().into(),
        timestamp: m["timestamp"].as_u64().unwrap(),
        proposal: m["proposal"].as_str().unwrap().into(),
        choice: m["choice"].as_u64().unwrap() as u32,
        reason: m["reason"].as_str().unwrap().into(),
        app: m["app"].as_str().unwrap().into(),
        metadata: m["metadata"].as_str().unwrap().into(),
    };
    // FINDINGS.md §8: the id the testnet sequencer returned and the hub shows.
    assert_eq!(snapshot::hex32(&vote.digest()), "0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657");
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

#[test]
fn reads_show_state_typed_data_verify_and_live_evidence() {
    let mut host = FakeHost::new()
        .signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000), SignScript::Sign])
        .posts(&[PostScript::Accept]);
    assert!(vote_document(&mut host, &scope(), "demo-1").unwrap().is_none());
    assert_eq!(new_document(&mut host, &scope()).unwrap()["last_write"], Value::Null);

    send(&mut host, &demo("demo-1"));
    host.calls.clear();
    let doc = vote_document(&mut host, &scope(), "demo-1").unwrap().unwrap();
    assert!(host.calls.is_empty(), "no hub call before signing");
    assert_eq!(doc["status"], "awaiting_approval");
    assert_eq!(doc["summary"]["choice_label"], "For");
    assert_eq!(doc["summary"]["proposal_title"], "bloom-ballot test");
    assert_eq!(doc["approval"]["pending"]["action_id"], EXACT_ID);
    let b = binding(&mut host, "demo-1").unwrap();
    assert_eq!(doc["typed_data"], b.typed_data);
    assert_eq!(doc["digest"], b.digest);
    assert_eq!(doc["verify"]["expected_out_hex"], b.digest);
    assert_eq!(
        doc["verify"]["commands"][0],
        "bloom vfs cat /petals/ballot/testnet/accounts/main/0/votes/demo-1.json | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-votes/in.json"
    );
    assert_eq!(doc["live"], Value::Null);

    send(&mut host, &demo("demo-1"));
    let doc = vote_document(&mut host, &scope(), "demo-1").unwrap().unwrap();
    assert_eq!(doc["status"], "submitted", "the read never writes the store");
    assert_eq!(doc["live"]["counted"], true);
    assert_eq!(doc["live"]["hub_vote"]["id"], b.digest);
    assert_eq!(record(&mut host, "demo-1").unwrap().status, Status::Submitted);
    host.hub_down = true;
    let doc = vote_document(&mut host, &scope(), "demo-1").unwrap().unwrap();
    assert_eq!(doc["live"]["checked"], false);
    assert!(doc["live"]["error"].as_str().unwrap().contains("502"));

    let new = new_document(&mut host, &scope()).unwrap();
    assert_eq!(new["last_write"]["outcome"], "submitted");
    assert_eq!(new["body_schema"]["max_bytes"], 4096);
    assert_secret_wall(&mut host, &["demo-1"]);
}

#[test]
fn refused_records_read_without_typed_data_and_list_with_bound_ones() {
    let mut host = FakeHost::new().signs(&[SignScript::Pending(EXACT_ID, NOW_MS + 300_000)]);
    send(&mut host, &body("bad-choice", P, 4, None));
    send(&mut host, &demo("demo-1"));
    let doc = vote_document(&mut host, &scope(), "bad-choice").unwrap().unwrap();
    assert_eq!(doc["status"], "refused");
    assert_eq!(doc["bound"], false);
    assert_eq!(doc["reason_code"], "choice_out_of_range");
    assert_eq!(doc["summary"]["choice"], 4);
    assert!(doc.get("typed_data").is_none() && doc.get("verify").is_none());
    assert_eq!(list_vote_ids(&mut host, &scope()).unwrap(), ["bad-choice", "demo-1"]);
    let other = Scope::new(Network::Testnet, "main", 1);
    assert!(list_vote_ids(&mut host, &other).unwrap().is_empty());
}
