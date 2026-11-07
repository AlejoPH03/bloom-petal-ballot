//! Stored documents of the vote flow (docs/plan-votes.md §1, §3, §9). None
//! of them has a field that can hold signature bytes; `signature_sha256` is
//! the only signature-derived value.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::snapshot::{self, Vote};

pub const BINDING_SCHEMA: &str = "ballot.vote-binding.v1";
pub const RECORD_SCHEMA: &str = "ballot.vote-record.v1";
pub const MARKER_SCHEMA: &str = "ballot.vote-last-write.v1";
/// The `app` every ballot vote carries (≤ 24 chars, FINDINGS.md §3).
pub const APP: &str = "bloom-ballot";
pub const METADATA: &str = "{}";
/// Most history entries kept in a record.
pub const HISTORY_MAX: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Refused,
    Drafted,
    AwaitingApproval,
    Signed,
    Submitted,
    Counted,
    RejectedBySnapshot,
    Expired,
    /// Reserved for the re-vote flow (Monday); never produced yet.
    Superseded,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Refused => "refused",
            Status::Drafted => "drafted",
            Status::AwaitingApproval => "awaiting_approval",
            Status::Signed => "signed",
            Status::Submitted => "submitted",
            Status::Counted => "counted",
            Status::RejectedBySnapshot => "rejected_by_snapshot",
            Status::Expired => "expired",
            Status::Superseded => "superseded",
        }
    }

    /// No further write changes this record.
    pub fn is_final(self) -> bool {
        matches!(self, Status::RejectedBySnapshot | Status::Expired | Status::Superseded)
    }
}

/// Hub facts about the proposal, frozen at draft time for the readable
/// summary. Times are the hub's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposalFacts {
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub choices: Vec<String>,
    pub choice_label: String,
    pub start: u64,
    pub end: u64,
    #[serde(default)]
    pub snapshot: Option<Value>,
}

/// The immutable binding of a vote id: written once with `put_new`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema: String,
    pub vote_id: String,
    pub network: String,
    pub wallet: String,
    pub index: u32,
    pub canonical_body_sha256: String,
    pub from: String,
    pub space: String,
    pub proposal: String,
    pub choice: u32,
    pub reason: String,
    pub timestamp: u64,
    pub app: String,
    pub metadata: String,
    pub proposal_facts: ProposalFacts,
    /// The frozen EIP-712 document (`Vote::typed_data`).
    pub typed_data: Value,
    /// `0x19 0x01 ‖ domainSeparator ‖ structHash`, 0x hex.
    pub preimage: String,
    /// keccak256(preimage), 0x hex: the signed hash and Snapshot's vote id.
    pub digest: String,
    pub drafted_ms: u64,
}

impl Binding {
    pub fn vote(&self) -> Vote {
        Vote {
            from: self.from.clone(),
            space: self.space.clone(),
            timestamp: self.timestamp,
            proposal: self.proposal.clone(),
            choice: self.choice,
            reason: self.reason.clone(),
            app: self.app.clone(),
            metadata: self.metadata.clone(),
        }
    }

    /// The stored digest, preimage and typed data must equal what the fields
    /// rebuild to; anything else is a corrupt binding.
    pub fn is_consistent(&self) -> bool {
        let vote = self.vote();
        self.schema == BINDING_SCHEMA
            && self.digest == snapshot::hex32(&vote.digest())
            && self.preimage == format!("0x{}", alloy_primitives::hex::encode(vote.preimage()))
            && self.typed_data == vote.typed_data()
    }
}

/// Where the owner finds a pending ceremony. The route cannot tell a
/// policy-update operation id from an exact request id (both 64 hex), so both
/// owner locations are named (as `hello::Pending` did).
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// Sequencer `id` (equals `digest` unless `warning` says otherwise).
    pub id: String,
    pub ipfs: String,
    pub relayer_address: String,
    /// sha256 of the relayer's receipt signature (the bytes are not kept).
    pub receipt_sha256: Option<String>,
    /// True when the acceptance was established from the hub after a lost or
    /// duplicate response, not from a sequencer response.
    pub reconciled: bool,
    pub warning: Option<String>,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountedEvidence {
    pub hub_vote_id: String,
    pub choice: u32,
    pub created: u64,
    pub checked_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub status: Status,
    pub at_ms: u64,
}

/// The mutable state of one vote id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: String,
    pub vote_id: String,
    pub status: Status,
    pub reason_code: Option<String>,
    pub detail: Option<String>,
    /// True when `reason_code` was inferred rather than reported (an expired
    /// approval reaches the route only as a bare denial).
    pub inferred: bool,
    /// The refused request (only for `refused`; a refused id is unbound).
    pub refused_body: Option<crate::votes::body::VoteBody>,
    pub pending: Option<Pending>,
    pub signature_sha256: Option<String>,
    pub submit_attempts: u32,
    pub last_attempt_ms: Option<u64>,
    pub receipt: Option<Receipt>,
    pub counted: Option<CountedEvidence>,
    pub last_error: Option<String>,
    pub history: Vec<HistoryEntry>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

impl Record {
    pub fn new(vote_id: &str, status: Status, now_ms: u64) -> Self {
        Self {
            schema: RECORD_SCHEMA.into(),
            vote_id: vote_id.to_owned(),
            status,
            reason_code: None,
            detail: None,
            inferred: false,
            refused_body: None,
            pending: None,
            signature_sha256: None,
            submit_attempts: 0,
            last_attempt_ms: None,
            receipt: None,
            counted: None,
            last_error: None,
            history: vec![HistoryEntry { status, at_ms: now_ms }],
            created_ms: now_ms,
            updated_ms: now_ms,
        }
    }

    /// Move to `status`, recording it in the history when it changes.
    pub fn set_status(&mut self, status: Status, now_ms: u64) {
        if self.status != status {
            self.status = status;
            self.history.push(HistoryEntry { status, at_ms: now_ms });
            if self.history.len() > HISTORY_MAX {
                let excess = self.history.len() - HISTORY_MAX;
                self.history.drain(1..=excess);
            }
        }
        self.updated_ms = now_ms;
    }
}

/// The last-write marker shown by `new.json`'s read: the only trace of a
/// mounted (asynchronous) write's outcome besides the record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub schema: String,
    pub at_ms: u64,
    /// sha256 of the raw body bytes.
    pub body_sha256: String,
    pub canonical_body_sha256: Option<String>,
    pub vote_id: Option<String>,
    pub outcome: String,
    /// 0 for success, otherwise the error code the write returned.
    pub code: i32,
    /// The exact text the write returned (empty on success).
    pub message: String,
    /// The record's file name under `votes/`, when there is one.
    pub record: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_names_and_finality() {
        let all = [
            Status::Refused,
            Status::Drafted,
            Status::AwaitingApproval,
            Status::Signed,
            Status::Submitted,
            Status::Counted,
            Status::RejectedBySnapshot,
            Status::Expired,
            Status::Superseded,
        ];
        for status in all {
            assert_eq!(serde_json::to_value(status).unwrap(), status.as_str());
        }
        let finals: Vec<_> = all.into_iter().filter(|s| s.is_final()).collect();
        assert_eq!(finals, [Status::RejectedBySnapshot, Status::Expired, Status::Superseded]);
    }

    #[test]
    fn history_tracks_changes_and_is_bounded() {
        let mut record = Record::new("v", Status::Drafted, 1);
        record.set_status(Status::Drafted, 2);
        assert_eq!(record.history.len(), 1, "same status adds nothing");
        assert_eq!(record.updated_ms, 2);
        for i in 0..100 {
            let status = if i % 2 == 0 { Status::AwaitingApproval } else { Status::Drafted };
            record.set_status(status, 10 + i);
        }
        assert_eq!(record.history.len(), HISTORY_MAX);
        assert_eq!(record.history[0], HistoryEntry { status: Status::Drafted, at_ms: 1 }, "first entry kept");
    }

    #[test]
    fn pending_names_both_owner_locations() {
        let id = "b".repeat(64);
        assert_eq!(
            Pending::new("main", &id, 5).owner_finds_ceremony_at,
            [
                format!("/petal-signing-requests/{id}.json"),
                format!("/wallets/main/policy-updates/pending/{id}/approval_challenge.json"),
            ]
        );
    }
}
