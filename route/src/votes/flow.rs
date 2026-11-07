//! The vote state machine (docs/plan-votes.md §3) and the read documents
//! (§8, §9), against the [`VoteHost`] trait so host tests run it with a
//! scripted fake. `votes/host.rs` is the only implementation that talks to
//! Bloom. Callers are generic over the host (never `dyn`), so a route that
//! only reads keeps only the host functions it calls, and its component only
//! the imports those need.
//!
//! Order of durable writes vs external calls (plan §3):
//! binding (`put_new`, frozen timestamp) → `sign_payload` → secret signature →
//! record `signed` → record `submit_attempts + 1` → POST → record `submitted` →
//! delete the secret. A retry on a `signed` record that already POSTed checks
//! the hub before posting again.

use serde_json::{Value, json};

use crate::Network;
use crate::mandate::Mandate;
use crate::hub::HubError;
use crate::proposals::{HubProposal, vote_type_supported, voting_open};
use crate::snapshot::{self, Vote};
use crate::votes::body::{VoteBody, body_schema, sha256_hex, vote_id_hint, MAX_BODY_BYTES, REASON_MAX_CHARS};
use crate::votes::hubq::{HubVote, evidence};
use crate::votes::record::{
    APP, BINDING_SCHEMA, Binding, CountedEvidence, MARKER_SCHEMA, METADATA, Marker, Pending, ProposalFacts, Receipt,
    Record, Status,
};
use crate::votes::sequencer::{PostResult, Rejection, SignatureBytes, Submission, classify, envelope_bytes};
use crate::votes::store::{KvStore, Lease, LeaseError, PutNew, Scope, get_json, put_json, put_new_json};

/// The Petal sign intent and Broker operation class.
pub const SIGN_INTENT: &str = "snapshot.vote";
/// The sequencer accepts a message timestamp at most 6 days old
/// (snapshot-sequencer `src/ingestor.ts:57-67`).
pub const SEQUENCER_MAX_AGE_SECS: u64 = 6 * 24 * 60 * 60;
/// A frozen timestamp this close to the sequencer's limit is treated as
/// expired before signing or posting.
pub const STALE_MARGIN_SECS: u64 = 10 * 60;
/// A denial this close to (or after) the pending approval's expiry is read as
/// an expired approval. Signer refused about 90 s early on our triad
/// (docs/known-limitations.md §4).
pub const APPROVAL_EXPIRY_MARGIN_MS: u64 = 120_000;
/// Bloom tool session used by the `verify` commands.
pub const VERIFY_SESSION: &str = "ballot-votes";

/// What `sign_payload` gave, as the flow needs it.
#[derive(Debug)]
pub enum SignResult {
    Signature(SignatureBytes),
    Pending { action_id: String, expires_ms: u64 },
    /// `SdkError::Host(HostStatus::Denied)`: Bloom drops the reason.
    Denied,
    Failed(String),
}

/// Everything the vote flow needs from the host.
pub trait VoteHost: KvStore {
    fn now_ms(&mut self) -> Option<u64>;
    fn random16(&mut self) -> Option<[u8; 16]>;
    /// The account's EVM address, EIP-55 checksummed.
    fn account_address(&mut self) -> Result<String, String>;
    fn hub_proposal(&mut self, network: Network, id: &str) -> Result<Option<HubProposal>, HubError>;
    fn hub_vp(&mut self, network: Network, voter: &str, space: &str, proposal: &str) -> Result<f64, HubError>;
    fn hub_votes(&mut self, network: Network, proposal: &str, voter: &str) -> Result<Vec<HubVote>, HubError>;
    /// One exact `snapshot.vote` signature over `vote` (derived nonce, no
    /// approval hint).
    fn sign(&mut self, vote: &Vote) -> SignResult;
    /// POST the envelope to the network's sequencer.
    fn post_vote(&mut self, network: Network, body: &[u8]) -> PostResult;
}

/// What a write returns: `code` 0 is success (`DispatchResponse::Write`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    pub code: i32,
    pub message: String,
    /// The marker's `outcome`.
    pub outcome: &'static str,
}

#[derive(Debug)]
struct Step {
    code: i32,
    outcome: &'static str,
    message: String,
    has_record: bool,
}

impl Step {
    fn ok(outcome: &'static str) -> Self {
        Self { code: 0, outcome, message: String::new(), has_record: true }
    }
    fn fail(code: i32, outcome: &'static str, message: impl Into<String>, has_record: bool) -> Self {
        Self { code, outcome, message: message.into(), has_record }
    }
}

/// The mandate's post-hub checks (docs/plan-mandate-inbox.md): blocked
/// keyword in the proposal's title or body, required reason, rolling daily
/// limit. A refusal is `(reason_code, detail)`. No mandate: every vote passes.
pub fn mandate_check(
    mandate: Option<&Mandate>,
    body: &VoteBody,
    proposal: &HubProposal,
    recent_votes: u32,
) -> Result<(), (&'static str, String)> {
    match mandate {
        Some(mandate) => mandate.check_draft(&proposal.title, &proposal.body, &body.reason, recent_votes),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

/// The `votes/new.json` write handler body.
pub fn run_write<H: VoteHost>(host: &mut H, scope: &Scope, body: &[u8]) -> WriteOutcome {
    let body_sha256 = sha256_hex(body);
    let mut vote_id = vote_id_hint(body);
    let mut canonical = None;
    let now_ms = host.now_ms().filter(|&ms| ms > 0);
    let step = match now_ms {
        None => Step::fail(-4, "transient_error", "clock unavailable; retry the same write", false),
        Some(now_ms) => match VoteBody::parse(body) {
            Err(message) => Step::fail(-3, "invalid_body", format!("invalid body: {message}"), false),
            Ok(parsed) => {
                vote_id = Some(parsed.vote_id.clone());
                canonical = Some(parsed.canonical_sha256());
                locked(host, scope, &parsed, now_ms)
            }
        },
    };
    let marker = Marker {
        schema: MARKER_SCHEMA.into(),
        at_ms: now_ms.unwrap_or(0),
        body_sha256,
        canonical_body_sha256: canonical,
        record: vote_id.as_ref().filter(|_| step.has_record).map(|id| format!("votes/{id}.json")),
        vote_id,
        outcome: step.outcome.into(),
        code: step.code,
        message: step.message.clone(),
    };
    // The record is authoritative; a marker failure does not change the
    // write's result (plan §9).
    let _ = put_json(host, &scope.marker_key(), &marker, "last-write marker");
    WriteOutcome { code: step.code, message: step.message, outcome: step.outcome }
}

fn locked<H: VoteHost>(host: &mut H, scope: &Scope, body: &VoteBody, now_ms: u64) -> Step {
    let Some(token) = host.random16() else {
        return Step::fail(-4, "transient_error", "random bytes unavailable; retry the same write", false);
    };
    let lease = match Lease::acquire(host, &scope.lease_key(&body.vote_id), token, now_ms) {
        Ok(lease) => lease,
        Err(LeaseError::Busy) => {
            return Step::fail(
                -3,
                "busy",
                format!("another write for vote_id {} is in progress; retry the same write", body.vote_id),
                false,
            );
        }
        Err(LeaseError::Store(error)) => return Step::fail(-4, "transient_error", format!("store: {error}"), false),
    };
    let step = resume_or_draft(host, scope, body, now_ms);
    lease.release(host);
    step
}

fn resume_or_draft<H: VoteHost>(host: &mut H, scope: &Scope, body: &VoteBody, now_ms: u64) -> Step {
    let binding = match get_json::<Binding>(host, &scope.binding_key(&body.vote_id), "vote binding") {
        Ok(binding) => binding,
        Err(error) => return store_failure(error),
    };
    let record = match get_json::<Record>(host, &scope.record_key(&body.vote_id), "vote record") {
        Ok(record) => record,
        Err(error) => return store_failure(error),
    };
    match binding {
        Some(binding) => bound(host, scope, body, binding, record, now_ms),
        None => draft(host, scope, body, now_ms),
    }
}

fn store_failure(error: String) -> Step {
    Step::fail(-4, "transient_error", format!("store: {error}; retry the same write"), true)
}

/// Save the record or turn the failure into the step to return.
fn save<H: VoteHost>(host: &mut H, scope: &Scope, record: &Record) -> Result<(), Step> {
    put_json(host, &scope.record_key(&record.vote_id), record, "vote record").map_err(store_failure)
}

fn refuse<H: VoteHost>(host: &mut H, scope: &Scope, body: &VoteBody, code: &str, detail: String, now_ms: u64) -> Step {
    let mut record = Record::new(&body.vote_id, Status::Refused, now_ms);
    record.reason_code = Some(code.into());
    record.detail = Some(detail.clone());
    record.refused_body = Some(body.clone());
    if let Err(step) = save(host, scope, &record) {
        return step;
    }
    Step::fail(-3, "refused", format!("refused: {code}: {detail}"), true)
}

fn transient(message: String, has_record: bool) -> Step {
    Step::fail(-4, "transient_error", message, has_record)
}

fn draft<H: VoteHost>(host: &mut H, scope: &Scope, body: &VoteBody, now_ms: u64) -> Step {
    let network = scope.network;
    if network == Network::Mainnet {
        // CLAUDE.md rule 7: no mainnet votes without the owner's explicit
        // authorization. Refused before any external call.
        return refuse(
            host,
            scope,
            body,
            "mainnet_disabled",
            "votes on mainnet are disabled in this build (see README, \"Enabling mainnet votes\")".into(),
            now_ms,
        );
    }
    // The owner's (advisory) mandate, read once. Its space rule is checked
    // before any hub call.
    let mandate = match crate::mandate::load(host, scope) {
        Ok(mandate) => mandate,
        Err(error) => return transient(format!("could not read the mandate: {error}; retry the same write"), false),
    };
    if let Some(Err((code, detail))) = mandate.as_ref().map(|mandate| mandate.check_space(&body.space)) {
        return refuse(host, scope, body, code, detail, now_ms);
    }
    let now = now_ms / 1000;
    let proposal = match host.hub_proposal(network, &body.proposal) {
        Ok(proposal) => proposal,
        Err(error) => return transient(format!("could not check the proposal: {}; retry the same write", error.message(network)), false),
    };
    let Some(proposal) = proposal else {
        return refuse(host, scope, body, "proposal_not_found", format!("the {} hub has no proposal {}", network.as_str(), body.proposal), now_ms);
    };
    let proposal_space = proposal.space.as_ref().map(|space| space.id.as_str()).unwrap_or("");
    if proposal_space != body.space {
        return refuse(host, scope, body, "space_mismatch", format!("the proposal belongs to space {proposal_space:?}, not {:?}", body.space), now_ms);
    }
    if !voting_open(&proposal.state, proposal.end, Some(now)) {
        return if proposal.state != "active" {
            refuse(host, scope, body, "proposal_not_active", format!("the proposal is {:?}, not active", proposal.state), now_ms)
        } else {
            refuse(host, scope, body, "voting_closed", format!("voting ended at {} (unix seconds)", proposal.end), now_ms)
        };
    }
    if now < proposal.start {
        return refuse(host, scope, body, "voting_not_started", format!("voting starts at {} (unix seconds)", proposal.start), now_ms);
    }
    if !vote_type_supported(&proposal.kind) {
        return refuse(host, scope, body, "unsupported_vote_type", format!("ballot votes only on single-choice and basic proposals, not {:?}", proposal.kind), now_ms);
    }
    let count = proposal.choices.len();
    if body.choice as usize > count {
        return refuse(host, scope, body, "choice_out_of_range", format!("choice {} but this proposal has {count} choices (1-{count})", body.choice), now_ms);
    }
    let from = match host.account_address() {
        Ok(from) => from,
        Err(error) => return transient(format!("could not read the account address: {error}"), false),
    };
    match host.hub_vp(network, &from, &body.space, &body.proposal) {
        Ok(vp) if vp > 0.0 => {}
        Ok(_) => return refuse(host, scope, body, "no_voting_power", format!("{from} has no voting power on this proposal"), now_ms),
        Err(error) => return transient(format!("could not check voting power: {}; retry the same write", error.message(network)), false),
    }
    let facts = ProposalFacts {
        title: proposal.title.clone(),
        kind: proposal.kind.clone(),
        choice_label: proposal.choices[body.choice as usize - 1].clone(),
        choices: proposal.choices.clone(),
        start: proposal.start,
        end: proposal.end,
        snapshot: proposal.snapshot.clone(),
    };
    let recent_votes = if mandate.as_ref().is_some_and(|mandate| mandate.max_votes_per_day.is_some()) {
        match crate::mandate::recent_vote_count(host, scope, now_ms) {
            Ok(count) => count,
            Err(error) => return transient(format!("could not count recent votes: {error}; retry the same write"), false),
        }
    } else {
        0
    };
    if let Err((code, detail)) = mandate_check(mandate.as_ref(), body, &proposal, recent_votes) {
        return refuse(host, scope, body, code, detail, now_ms);
    }
    let binding = freeze(scope, body, &from, facts, now, now_ms);
    match put_new_json(host, &scope.binding_key(&body.vote_id), &binding, "vote binding") {
        Ok(PutNew::Created) => {}
        // Bound by a concurrent write in between: resume from what is stored.
        Ok(PutNew::Exists) => return resume_or_draft(host, scope, body, now_ms),
        Err(error) => return store_failure(error),
    }
    let record = Record::new(&body.vote_id, Status::Drafted, now_ms);
    if let Err(step) = save(host, scope, &record) {
        return step;
    }
    sign_stage(host, scope, &binding, record, now_ms)
}

/// Freeze the vote: the timestamp is fixed here, once, before any signature.
fn freeze(scope: &Scope, body: &VoteBody, from: &str, facts: ProposalFacts, now: u64, now_ms: u64) -> Binding {
    let vote = Vote {
        from: from.to_owned(),
        space: body.space.clone(),
        timestamp: now,
        proposal: body.proposal.clone(),
        choice: body.choice,
        reason: body.reason.clone(),
        app: APP.into(),
        metadata: METADATA.into(),
    };
    Binding {
        schema: BINDING_SCHEMA.into(),
        vote_id: body.vote_id.clone(),
        network: scope.network.as_str().into(),
        wallet: scope.wallet.clone(),
        index: scope.index,
        canonical_body_sha256: body.canonical_sha256(),
        from: vote.from.clone(),
        space: vote.space.clone(),
        proposal: vote.proposal.clone(),
        choice: vote.choice,
        reason: vote.reason.clone(),
        timestamp: vote.timestamp,
        app: vote.app.clone(),
        metadata: vote.metadata.clone(),
        proposal_facts: facts,
        typed_data: vote.typed_data(),
        preimage: format!("0x{}", alloy_primitives::hex::encode(vote.preimage())),
        digest: snapshot::hex32(&vote.digest()),
        drafted_ms: now_ms,
    }
}

fn bound<H: VoteHost>(
    host: &mut H,
    scope: &Scope,
    body: &VoteBody,
    binding: Binding,
    record: Option<Record>,
    now_ms: u64,
) -> Step {
    if !binding.is_consistent() {
        return Step::fail(-4, "transient_error", "the stored vote binding is inconsistent; not signing", true);
    }
    if !body.same_vote(&binding.space, &binding.proposal, binding.choice, &binding.reason) {
        return Step::fail(
            -3,
            "conflict",
            format!(
                "vote_id {} is bound to a different vote (space, proposal, choice or reason differ); use a new vote_id",
                body.vote_id
            ),
            true,
        );
    }
    // A binding without a record (crash right after binding) is a draft. A
    // refused record never coexists with a binding; if it did, the binding wins.
    let record = record
        .filter(|record| record.status != Status::Refused)
        .unwrap_or_else(|| Record::new(&body.vote_id, Status::Drafted, now_ms));
    match record.status {
        Status::Refused | Status::Drafted | Status::AwaitingApproval => sign_stage(host, scope, &binding, record, now_ms),
        Status::Signed => submit_stage(host, scope, &binding, record, None, now_ms),
        Status::Submitted => count_stage(host, scope, &binding, record, now_ms),
        Status::Counted => Step::ok("counted"),
        Status::RejectedBySnapshot | Status::Expired | Status::Superseded => {
            let outcome = match record.status {
                Status::RejectedBySnapshot => "rejected_by_snapshot",
                Status::Expired => "expired",
                _ => "superseded",
            };
            Step::fail(
                -3,
                outcome,
                format!(
                    "{} ({}): this vote_id is final; write a new vote_id to vote again",
                    record.status.as_str(),
                    record.reason_code.as_deref().unwrap_or("-")
                ),
                true,
            )
        }
    }
}

/// `(reason_code, detail)` when the frozen vote can no longer be accepted.
pub fn expiry(binding: &Binding, now_ms: u64) -> Option<(&'static str, String)> {
    let now = now_ms / 1000;
    if now >= binding.proposal_facts.end {
        return Some(("proposal_closed", format!("voting ended at {} (unix seconds)", binding.proposal_facts.end)));
    }
    if now.saturating_sub(binding.timestamp) > SEQUENCER_MAX_AGE_SECS - STALE_MARGIN_SECS {
        return Some((
            "timestamp_stale",
            format!("the vote's timestamp {} is too old for the Snapshot sequencer (6 days)", binding.timestamp),
        ));
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn finalize<H: VoteHost>(
    host: &mut H,
    scope: &Scope,
    mut record: Record,
    status: Status,
    code: &str,
    detail: String,
    inferred: bool,
    now_ms: u64,
) -> Step {
    record.reason_code = Some(code.into());
    record.detail = Some(detail.clone());
    record.inferred = inferred;
    record.pending = None;
    record.set_status(status, now_ms);
    let _ = host.secret_del(&scope.signature_key(&record.vote_id));
    if let Err(step) = save(host, scope, &record) {
        return step;
    }
    let (outcome, head) = match status {
        Status::Expired => ("expired", "expired"),
        _ => ("rejected_by_snapshot", "rejected by Snapshot"),
    };
    Step::fail(
        -3,
        outcome,
        format!("{head}: {code}: {detail}; this vote_id is final; write a new vote_id to vote again"),
        true,
    )
}

const DENIED_TEXT: &str = "signing refused: missing installer provenance, a wallet policy that does not allow this package, an approval that is not active, or another live ceremony on this wallet (CEREMONY_RATE_LIMITED; finish it first). See Machine's petal.sign_payload_denied log line, then retry the same write.";

fn sign_stage<H: VoteHost>(host: &mut H, scope: &Scope, binding: &Binding, mut record: Record, now_ms: u64) -> Step {
    if let Some((code, detail)) = expiry(binding, now_ms) {
        return finalize(host, scope, record, Status::Expired, code, detail, false, now_ms);
    }
    let vote = binding.vote();
    match host.sign(&vote) {
        SignResult::Pending { action_id, expires_ms } => {
            let message = format!(
                "approval pending ({action_id}); the owner opens the ceremony from /petal-signing-requests/{action_id}.json or /wallets/{w}/policy-updates/pending/{action_id}/approval_challenge.json, checks that its exact hash equals this vote's digest {d} (run the verify commands in votes/{id}.json), approves, then retry the exact same write",
                w = scope.wallet,
                d = binding.digest,
                id = binding.vote_id,
            );
            record.pending = Some(Pending::new(&scope.wallet, &action_id, expires_ms));
            record.last_error = None;
            record.set_status(Status::AwaitingApproval, now_ms);
            if let Err(step) = save(host, scope, &record) {
                return step;
            }
            Step::fail(-3, "approval_pending", message, true)
        }
        SignResult::Signature(signature) => {
            let recovers = snapshot::recover_signer(&vote.digest(), signature.as_slice())
                .ok()
                .is_some_and(|address| address.to_checksum(None) == binding.from);
            if !recovers {
                drop(signature);
                record.last_error = Some("Bloom returned a signature that does not recover to this account".into());
                record.updated_ms = now_ms;
                if let Err(step) = save(host, scope, &record) {
                    return step;
                }
                return transient("Bloom returned a signature that does not recover to this account".into(), true);
            }
            if let Err(error) = host.secret_put(&scope.signature_key(&binding.vote_id), signature.as_slice()) {
                return transient(format!("could not keep the signature: {error}; retry the same write"), true);
            }
            record.signature_sha256 = Some(signature.fingerprint());
            record.pending = None;
            record.last_error = None;
            record.set_status(Status::Signed, now_ms);
            if let Err(step) = save(host, scope, &record) {
                return step;
            }
            submit_stage(host, scope, binding, record, Some(signature), now_ms)
        }
        SignResult::Denied => {
            let expired = record.status == Status::AwaitingApproval
                && record
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.expires_ms > 0 && now_ms + APPROVAL_EXPIRY_MARGIN_MS >= pending.expires_ms);
            if expired {
                return finalize(
                    host,
                    scope,
                    record,
                    Status::Expired,
                    "approval_expired",
                    "inferred: Bloom denied the signature at or near the end of the owner approval window (it reports only a denial)".into(),
                    true,
                    now_ms,
                );
            }
            record.last_error = Some(DENIED_TEXT.into());
            record.updated_ms = now_ms;
            if let Err(step) = save(host, scope, &record) {
                return step;
            }
            Step::fail(-3, "signing_refused", DENIED_TEXT, true)
        }
        SignResult::Failed(message) => {
            let message = format!("signing failed: {}", crate::votes::sequencer::redact(&message));
            record.last_error = Some(message.clone());
            record.updated_ms = now_ms;
            if let Err(step) = save(host, scope, &record) {
                return step;
            }
            transient(format!("{message}; retry the same write"), true)
        }
    }
}

/// The hub's vote for this voter on this proposal, when its id is our digest.
fn hub_shows<H: VoteHost>(host: &mut H, scope: &Scope, binding: &Binding) -> Result<Option<HubVote>, String> {
    let votes = host
        .hub_votes(scope.network, &binding.proposal, &binding.from)
        .map_err(|error| error.message(scope.network))?;
    Ok(evidence(&votes, &binding.digest, binding.choice, binding.timestamp)
        .hub_vote
        .filter(|vote| vote.id.eq_ignore_ascii_case(&binding.digest)))
}

fn mark_submitted<H: VoteHost>(host: &mut H, scope: &Scope, mut record: Record, receipt: Receipt, now_ms: u64) -> Step {
    let id = receipt.id.clone();
    record.receipt = Some(receipt);
    record.pending = None;
    record.last_error = None;
    record.set_status(Status::Submitted, now_ms);
    if let Err(error) = put_json(host, &scope.record_key(&record.vote_id), &record, "vote record") {
        return transient(
            format!("Snapshot accepted the vote (id {id}) but the record could not be stored: {error}; retry the same write"),
            true,
        );
    }
    let _ = host.secret_del(&scope.signature_key(&record.vote_id));
    Step::ok("submitted")
}

fn reconciled_receipt(vote: &HubVote, now_ms: u64) -> Receipt {
    Receipt {
        id: vote.id.clone(),
        ipfs: String::new(),
        relayer_address: String::new(),
        receipt_sha256: None,
        reconciled: true,
        warning: None,
        at_ms: now_ms,
    }
}

fn submit_stage<H: VoteHost>(
    host: &mut H,
    scope: &Scope,
    binding: &Binding,
    mut record: Record,
    signature: Option<SignatureBytes>,
    now_ms: u64,
) -> Step {
    if record.submit_attempts > 0 {
        match hub_shows(host, scope, binding) {
            Ok(Some(vote)) => return mark_submitted(host, scope, record, reconciled_receipt(&vote, now_ms), now_ms),
            Ok(None) => {}
            Err(error) => {
                return transient(
                    format!("could not confirm the previous submission on the hub ({error}); not posting again; retry the same write"),
                    true,
                );
            }
        }
    }
    if let Some((code, detail)) = expiry(binding, now_ms) {
        return finalize(host, scope, record, Status::Expired, code, detail, false, now_ms);
    }
    let signature = match signature {
        Some(signature) => signature,
        None => match host.secret_get(&scope.signature_key(&binding.vote_id), 1024) {
            Ok(Some(bytes)) => SignatureBytes::new(bytes),
            Ok(None) => {
                // The kept signature is gone: sign the same frozen payload
                // again (same request id; Machine recovers or re-signs).
                record.signature_sha256 = None;
                record.set_status(Status::Drafted, now_ms);
                if let Err(step) = save(host, scope, &record) {
                    return step;
                }
                return sign_stage(host, scope, binding, record, now_ms);
            }
            Err(error) => return transient(format!("could not read the kept signature: {error}; retry the same write"), true),
        },
    };
    record.submit_attempts += 1;
    record.last_attempt_ms = Some(now_ms);
    record.updated_ms = now_ms;
    if let Err(step) = save(host, scope, &record) {
        return step;
    }
    let vote = binding.vote();
    let result = host.post_vote(scope.network, &envelope_bytes(&vote, &signature));
    drop(signature);
    match classify(&result) {
        Submission::Accepted(accepted) => {
            let warning = (!accepted.id.eq_ignore_ascii_case(&binding.digest)).then(|| {
                format!("receipt_id_mismatch: the sequencer id {} differs from the digest {}", accepted.id, binding.digest)
            });
            let receipt = Receipt {
                id: accepted.id,
                ipfs: accepted.ipfs,
                relayer_address: accepted.relayer_address,
                receipt_sha256: accepted.receipt_sha256,
                reconciled: false,
                warning,
                at_ms: now_ms,
            };
            mark_submitted(host, scope, record, receipt, now_ms)
        }
        Submission::Duplicate => match hub_shows(host, scope, binding) {
            Ok(Some(vote)) => mark_submitted(host, scope, record, reconciled_receipt(&vote, now_ms), now_ms),
            other => {
                let why = match other {
                    Err(error) => format!("the hub check failed ({error})"),
                    _ => "the hub does not show it yet".into(),
                };
                let message = format!("the sequencer reports a duplicate but {why}; retry the same write");
                record.last_error = Some(message.clone());
                if let Err(step) = save(host, scope, &record) {
                    return step;
                }
                transient(message, true)
            }
        },
        Submission::Transient(message) => {
            record.last_error = Some(message.clone());
            if let Err(step) = save(host, scope, &record) {
                return step;
            }
            transient(format!("submission not confirmed: {message}; retry the same write"), true)
        }
        Submission::Permanent(rejection) => {
            let now = now_ms / 1000;
            match rejection {
                Rejection::NotInVotingWindow if now >= binding.proposal_facts.end => finalize(
                    host,
                    scope,
                    record,
                    Status::Expired,
                    "proposal_closed",
                    "the sequencer reports the voting window has ended".into(),
                    false,
                    now_ms,
                ),
                Rejection::WrongTimestamp if now.saturating_sub(binding.timestamp) > SEQUENCER_MAX_AGE_SECS => finalize(
                    host,
                    scope,
                    record,
                    Status::Expired,
                    "timestamp_stale",
                    "the sequencer refused the vote's timestamp as too old".into(),
                    false,
                    now_ms,
                ),
                Rejection::WrongTimestamp => finalize(
                    host,
                    scope,
                    record,
                    Status::RejectedBySnapshot,
                    "clock_skew",
                    "the sequencer refused the vote's timestamp (more than 5 minutes ahead of its clock)".into(),
                    false,
                    now_ms,
                ),
                other => finalize(
                    host,
                    scope,
                    record,
                    Status::RejectedBySnapshot,
                    other.code(),
                    "the Snapshot sequencer refused the vote".into(),
                    false,
                    now_ms,
                ),
            }
        }
    }
}

fn count_stage<H: VoteHost>(host: &mut H, scope: &Scope, binding: &Binding, mut record: Record, now_ms: u64) -> Step {
    let _ = host.secret_del(&scope.signature_key(&binding.vote_id));
    let Ok(votes) = host.hub_votes(scope.network, &binding.proposal, &binding.from) else {
        return Step::ok("submitted");
    };
    let found = evidence(&votes, &binding.digest, binding.choice, binding.timestamp);
    match found.hub_vote.filter(|_| found.counted) {
        Some(vote) => {
            record.counted = Some(CountedEvidence {
                hub_vote_id: vote.id.clone(),
                choice: binding.choice,
                created: vote.created,
                checked_ms: now_ms,
            });
            record.set_status(Status::Counted, now_ms);
            if let Err(step) = save(host, scope, &record) {
                return step;
            }
            Step::ok("counted")
        }
        None => Step::ok("submitted"),
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// The `votes/new.json` read: body schema, rules and the last-write marker.
pub fn new_document<H: KvStore>(host: &mut H, scope: &Scope) -> Result<Value, String> {
    let marker: Option<Marker> = get_json(host, &scope.marker_key(), "last-write marker")?;
    let dir = scope.votes_dir();
    Ok(json!({
        "schema": "ballot.votes-new.v1",
        "network": scope.network.as_str(),
        "wallet": scope.wallet,
        "index": scope.index,
        "sign_intent": SIGN_INTENT,
        "write": format!("bloom vfs write {dir}/new.json --data '{{\"vote_id\":\"<id>\",\"space\":\"<space>\",\"proposal\":\"0x…\",\"choice\":1,\"reason\":\"…\"}}'"),
        "body_schema": body_schema(),
        "rules": {
            "body": format!("JSON object, at most {MAX_BODY_BYTES} bytes, unknown fields refused; reason at most {REASON_MAX_CHARS} characters (missing = \"\"); vote_id \"new\" is reserved"),
            "binding": "a vote_id binds permanently to (network, account, space, proposal, choice, reason) and the frozen typed data once drafted. The identical body resumes from the current state; a different body under a bound vote_id is refused (-3). A refused vote_id is not bound: a corrected body may reuse it.",
            "checks_before_signing": "the proposal exists on this network and belongs to space; it is active and now < end; type single-choice or basic; 1 <= choice <= number of choices; the account has voting power. Each failure is recorded as refused with a reason_code.",
            "states": ["refused", "drafted", "awaiting_approval", "signed", "submitted", "counted", "rejected_by_snapshot", "expired", "superseded"],
            "approval": "a write that needs the owner returns -3 'approval pending (<id>)'. Before approving, the owner runs the verify commands in votes/<vote_id>.json and checks that Bloom's hash equals the ceremony's exact hash; after approving, retry the exact same write promptly (the approval window is 5 minutes).",
            "completion": "a successful write means the handler finished: status submitted means the Snapshot sequencer accepted the vote; counted means the hub shows it. Read votes/<vote_id>.json.",
            "expiry": "expired (proposal_closed, timestamp_stale, approval_expired) and rejected_by_snapshot are final: write a new vote_id to vote again.",
            "read_after_write": "every write, including mounted (asynchronous) ones, updates last_write below; compare its body_sha256 with sha256 of your body.",
            "mainnet": "mainnet votes are disabled in this build (refused as mainnet_disabled)."
        },
        "last_write": marker,
    }))
}

/// The `votes/[vote_id].json` read. `None` when the id has no record.
pub fn vote_document<H: VoteHost>(host: &mut H, scope: &Scope, vote_id: &str) -> Result<Option<Value>, String> {
    let binding: Option<Binding> = get_json(host, &scope.binding_key(vote_id), "vote binding")?;
    let record: Option<Record> = get_json(host, &scope.record_key(vote_id), "vote record")?;
    if binding.is_none() && record.is_none() {
        return Ok(None);
    }
    let path = format!("{}/{vote_id}.json", scope.votes_dir());
    let status = match (&binding, &record) {
        (Some(_), Some(record)) if record.status != Status::Refused => record.status,
        (Some(_), _) => Status::Drafted,
        (None, Some(record)) => record.status,
        (None, None) => unreachable!(),
    };
    let mut doc = json!({
        "schema": "ballot.vote.v1",
        "vote_id": vote_id,
        "network": scope.network.as_str(),
        "wallet": scope.wallet,
        "index": scope.index,
        "sign_intent": SIGN_INTENT,
        "status": status.as_str(),
        "bound": binding.is_some(),
        "next": next_step(status),
        "note": "A write is never a completed vote: submitted means the Snapshot sequencer accepted it; counted means the hub shows a vote whose id equals this digest with this choice.",
    });
    let map = doc.as_object_mut().expect("object");
    if let Some(record) = &record {
        for (key, value) in [
            ("reason_code", json!(record.reason_code)),
            ("detail", json!(record.detail)),
            ("inferred", json!(record.inferred)),
            ("last_error", json!(record.last_error)),
            ("approval", json!(record.pending.as_ref().map(|pending| json!({
                "pending": pending,
                "what_to_do": "the owner opens the ceremony from the location that exists, compares its exact hash with verify.expected_out_hex, approves, then the agent retries the identical write promptly",
            })))),
            ("signature_sha256", json!(record.signature_sha256)),
            ("submit_attempts", json!(record.submit_attempts)),
            ("last_attempt_ms", json!(record.last_attempt_ms)),
            ("receipt", json!(record.receipt)),
            ("counted", json!(record.counted)),
            ("history", json!(record.history)),
            ("created_ms", json!(record.created_ms)),
            ("updated_ms", json!(record.updated_ms)),
        ] {
            map.insert(key.into(), value);
        }
    }
    match &binding {
        Some(binding) => {
            let facts = &binding.proposal_facts;
            map.insert("summary".into(), json!({
                "space": binding.space,
                "proposal": binding.proposal,
                "proposal_title": facts.title,
                "type": facts.kind,
                "choice": binding.choice,
                "choice_label": facts.choice_label,
                "choices": crate::proposals::number_choices(&facts.choices),
                "reason": binding.reason,
                "from": binding.from,
                "timestamp": binding.timestamp,
                "voting_start": facts.start,
                "voting_end": facts.end,
                "app": binding.app,
            }));
            map.insert("typed_data".into(), binding.typed_data.clone());
            map.insert("preimage".into(), json!(binding.preimage));
            map.insert("digest".into(), json!(binding.digest));
            map.insert("verify".into(), verify_section(&path, &binding.digest));
            let live = if matches!(status, Status::Signed | Status::Submitted | Status::Counted) {
                live_check(host, scope, binding)
            } else {
                Value::Null
            };
            map.insert("live".into(), live);
        }
        None => {
            if let Some(body) = record.as_ref().and_then(|record| record.refused_body.as_ref()) {
                map.insert("summary".into(), json!({
                    "space": body.space,
                    "proposal": body.proposal,
                    "choice": body.choice,
                    "reason": body.reason,
                }));
            }
        }
    }
    Ok(Some(doc))
}

fn next_step(status: Status) -> &'static str {
    match status {
        Status::Refused => "fix the request (see reason_code) and write again; this vote_id is not bound",
        Status::Drafted => "retry the identical write",
        Status::AwaitingApproval => "the owner verifies the digest and approves the ceremony; then retry the identical write promptly",
        Status::Signed => "retry the identical write: it checks the hub, then submits",
        Status::Submitted => "retry the identical write later to record counted once the hub shows the vote (see live.counted)",
        Status::Counted => "done",
        Status::RejectedBySnapshot | Status::Expired | Status::Superseded => {
            "final: write a new vote_id to vote again"
        }
    }
}

/// Exact commands for the owner to recompute the digest with Bloom's own
/// EIP-712 tool (`bloom-vfs/src/handlers/tools.rs:17`, `539-545`), without
/// trusting Ballot's hashing (as Step 1's hello-sign.json did).
pub fn verify_section(path: &str, digest: &str) -> Value {
    json!({
        "why": "The ceremony shows only the digest. Recompute it from the readable typed_data with Bloom's own tool and compare before approving.",
        "commands": [
            format!("bloom vfs cat {path} | jq -c .typed_data | bloom vfs write /tools/eip712/hash/{VERIFY_SESSION}/in.json"),
            format!("bloom vfs cat /tools/eip712/hash/{VERIFY_SESSION}/out.hex"),
        ],
        "expected_out_hex": digest,
        "compare_with": "the exact hash in the ceremony's \"The exact plan that was reviewed\" (exact_ordered_hashes; shown there without 0x)",
        "after_submission": "the same value is Snapshot's vote id (receipt.id and the hub's vote id)",
    })
}

/// Live hub evidence for the read (no store write).
fn live_check<H: VoteHost>(host: &mut H, scope: &Scope, binding: &Binding) -> Value {
    let checked_at_ms = host.now_ms();
    match host.hub_votes(scope.network, &binding.proposal, &binding.from) {
        Ok(votes) => {
            let found = evidence(&votes, &binding.digest, binding.choice, binding.timestamp);
            json!({
                "checked": true,
                "checked_at_ms": checked_at_ms,
                "counted": found.counted,
                "hub_vote": found.hub_vote,
                "superseded_hint": found.superseded_hint,
            })
        }
        Err(error) => json!({
            "checked": false,
            "checked_at_ms": checked_at_ms,
            "error": error.message(scope.network),
        }),
    }
}

/// Vote ids with a record or binding, for the `votes/` listing.
pub fn list_vote_ids<H: KvStore>(host: &mut H, scope: &Scope) -> Result<Vec<String>, String> {
    let keys = host.store_list(&scope.votes_prefix(), crate::votes::store::MAX_LIST_BYTES)?;
    Ok(scope.vote_ids_from_keys(&keys))
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
