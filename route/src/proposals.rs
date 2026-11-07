//! Snapshot proposal queries and their read projections.
//!
//! The query texts are verified against both hubs and recorded with their
//! responses in `docs/snapshot-testnet/responses/ballot_*` (2026-11-07).
//! Times are always the hub's values: the hub stores `start` 1 s after the
//! signed value (FINDINGS.md §9), so nothing here recomputes them.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::hub::{HubQuery, truncate_utf8};
use crate::{Network, ValidationError, validate_proposal_id, validate_space};

/// A space's active proposals (up to 100) and its 10 most recent ones.
pub const PROPOSALS_QUERY: &str = "query Proposals($space: String!) { active: proposals(first: 100, where: {space: $space, state: \"active\"}, orderBy: \"created\", orderDirection: desc) { id title type choices start end state scores_total created } recent: proposals(first: 10, where: {space: $space}, orderBy: \"created\", orderDirection: desc) { id title type choices start end state scores_total created } }";
/// One proposal by id.
pub const PROPOSAL_QUERY: &str = "query Proposal($id: String!) { proposal(id: $id) { id space { id } author title body choices type start end snapshot state scores scores_total votes } }";

/// Largest accepted hub response for the space listing.
pub const MAX_PROPOSALS_RESPONSE_BYTES: usize = 512 * 1024;
/// Largest accepted hub response for one proposal.
pub const MAX_PROPOSAL_RESPONSE_BYTES: usize = 256 * 1024;
/// A proposal body is cut to this many bytes, on a UTF-8 boundary.
pub const BODY_MAX_BYTES: usize = 8192;

/// The listing query for a validated space id.
pub fn proposals_query(space: &str) -> Result<HubQuery, ValidationError> {
    let space = validate_space(space)?;
    Ok(HubQuery::new(PROPOSALS_QUERY, json!({ "space": space }), MAX_PROPOSALS_RESPONSE_BYTES))
}

/// The single-proposal query for a validated proposal id.
pub fn proposal_query(id: &str) -> Result<HubQuery, ValidationError> {
    let id = validate_proposal_id(id)?;
    Ok(HubQuery::new(PROPOSAL_QUERY, json!({ "id": id }), MAX_PROPOSAL_RESPONSE_BYTES))
}

/// Voting types ballot can sign: `voteTypes` with a single `uint32` choice
/// (FINDINGS.md §2). The other types sign `uint32[]` or a JSON string.
pub fn vote_type_supported(kind: &str) -> bool {
    matches!(kind, "single-choice" | "basic")
}

/// Whether a vote may be cast now: the hub says `active` and `now` (seconds)
/// is before the hub's `end`. Unknown time (`None`) is never open. Shared by
/// the read routes and the vote write's pre-signing checks; the sequencer
/// enforces the window itself (snapshot-sequencer `writer/vote.ts:30-38`).
pub fn voting_open(state: &str, end: u64, now: Option<u64>) -> bool {
    state == "active" && now.is_some_and(|now| now < end)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// 1-based, as Snapshot numbers single-choice and basic choices.
    pub number: u32,
    pub label: String,
}

pub fn number_choices(choices: &[String]) -> Vec<Choice> {
    (1..)
        .zip(choices)
        .map(|(number, label)| Choice { number, label: label.clone() })
        .collect()
}

/// One proposal as the hub returns it for the listing query. Fields the
/// query asks for are optional so older recordings still decode.
#[derive(Clone, Debug, Deserialize)]
pub struct HubSummary {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub start: u64,
    #[serde(default)]
    pub end: u64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub scores_total: Option<f64>,
    #[serde(default)]
    pub created: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ProposalsData {
    #[serde(default)]
    pub active: Vec<HubSummary>,
    #[serde(default)]
    pub recent: Vec<HubSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProposalSummary {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub choices: Vec<Choice>,
    pub start: u64,
    pub end: u64,
    pub state: String,
    pub scores_total: Option<f64>,
    pub vote_type_supported: bool,
    pub voting_open: bool,
}

impl ProposalSummary {
    fn project(raw: HubSummary, now: Option<u64>) -> Self {
        Self {
            vote_type_supported: vote_type_supported(&raw.kind),
            voting_open: voting_open(&raw.state, raw.end, now),
            choices: number_choices(&raw.choices),
            id: raw.id,
            title: raw.title,
            kind: raw.kind,
            start: raw.start,
            end: raw.end,
            state: raw.state,
            scores_total: raw.scores_total,
        }
    }
}

/// Active proposals plus recent ones, each id once, newest first (by
/// `created`, falling back to `start`; ties by id for a stable order).
pub fn merge_proposals(active: Vec<HubSummary>, recent: Vec<HubSummary>, now: Option<u64>) -> Vec<ProposalSummary> {
    let mut seen = HashSet::new();
    let mut merged: Vec<HubSummary> = active
        .into_iter()
        .chain(recent)
        .filter(|proposal| seen.insert(proposal.id.clone()))
        .collect();
    merged.sort_by(|a, b| {
        let key = |p: &HubSummary| p.created.unwrap_or(p.start);
        key(b).cmp(&key(a)).then_with(|| a.id.cmp(&b.id))
    });
    merged.into_iter().map(|raw| ProposalSummary::project(raw, now)).collect()
}

/// The `proposals.json` document.
pub fn proposals_document(network: Network, space: &str, data: ProposalsData, fetched_at: Option<u64>) -> Value {
    json!({
        "network": network.as_str(),
        "space": space,
        "fetched_at": fetched_at,
        "proposals": merge_proposals(data.active, data.recent, fetched_at),
    })
}

#[derive(Clone, Debug, Deserialize)]
pub struct HubSpaceRef {
    pub id: String,
}

/// One proposal as the hub returns it for the single-proposal query.
#[derive(Clone, Debug, Deserialize)]
pub struct HubProposal {
    pub id: String,
    #[serde(default)]
    pub space: Option<HubSpaceRef>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub start: u64,
    #[serde(default)]
    pub end: u64,
    /// A block number; the hub's schema allows a string, so keep it as given.
    #[serde(default)]
    pub snapshot: Option<Value>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub scores: Vec<f64>,
    #[serde(default)]
    pub scores_total: Option<f64>,
    #[serde(default)]
    pub votes: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ProposalData {
    pub proposal: Option<HubProposal>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProposalDetail {
    pub network: &'static str,
    pub fetched_at: Option<u64>,
    pub id: String,
    pub space: Option<String>,
    pub author: String,
    pub title: String,
    pub body: String,
    pub truncated: bool,
    pub choices: Vec<Choice>,
    #[serde(rename = "type")]
    pub kind: String,
    pub start: u64,
    pub end: u64,
    pub snapshot: Option<Value>,
    pub state: String,
    pub scores: Vec<f64>,
    pub scores_total: Option<f64>,
    pub votes: Option<u64>,
    pub vote_type_supported: bool,
    pub voting_open: bool,
}

pub fn project_proposal(network: Network, raw: HubProposal, fetched_at: Option<u64>) -> ProposalDetail {
    let (body, truncated) = truncate_utf8(&raw.body, BODY_MAX_BYTES);
    let snapshot = raw.snapshot.filter(|value| value.is_number() || value.is_string());
    ProposalDetail {
        network: network.as_str(),
        fetched_at,
        body: body.to_owned(),
        truncated,
        choices: number_choices(&raw.choices),
        vote_type_supported: vote_type_supported(&raw.kind),
        voting_open: voting_open(&raw.state, raw.end, fetched_at),
        space: raw.space.map(|space| space.id),
        id: raw.id,
        author: raw.author,
        title: raw.title,
        kind: raw.kind,
        start: raw.start,
        end: raw.end,
        snapshot,
        state: raw.state,
        scores: raw.scores,
        scores_total: raw.scores_total,
        votes: raw.votes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{HubError, parse_response};

    macro_rules! recorded {
        ($name:literal) => {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/snapshot-testnet/responses/", $name))
        };
    }

    const DEMO_ID: &str = "0x5fa4f371c47a9691128eaa0bc079abed859f73b9f2e0afb453ff5a7d5b752607";
    const CLOSED_ID: &str = "0xb2a3f2fba4445f4672c2b2151ba32675ca5e3144f22910205ea1b159300425f2";

    #[test]
    fn query_texts_are_the_recorded_ones() {
        assert_eq!(PROPOSALS_QUERY.as_bytes(), recorded!("ballot_proposals.graphql"));
        assert_eq!(PROPOSAL_QUERY.as_bytes(), recorded!("ballot_proposal.graphql"));
    }

    #[test]
    fn query_variables_carry_the_validated_values() {
        let list = proposals_query("bloom-ballot-182c.eth").unwrap();
        assert_eq!(list.query(), PROPOSALS_QUERY);
        assert_eq!(list.variables(), &json!({"space": "bloom-ballot-182c.eth"}));
        assert_eq!(list.max_bytes(), MAX_PROPOSALS_RESPONSE_BYTES);
        let one = proposal_query(DEMO_ID).unwrap();
        assert_eq!(one.query(), PROPOSAL_QUERY);
        assert_eq!(one.variables(), &json!({"id": DEMO_ID}));
        assert_eq!(one.max_bytes(), MAX_PROPOSAL_RESPONSE_BYTES);
        // The value never appears in the query text, only in variables.
        let body: Value = serde_json::from_slice(&one.body()).unwrap();
        assert!(!body["query"].as_str().unwrap().contains(DEMO_ID));
        assert_eq!(body["variables"]["id"], DEMO_ID);
    }

    #[test]
    fn query_constructors_reject_invalid_values() {
        for bad in ["NOT_VALID", "", "x", "a.eth\" } evil { x", "ens.eth/../x"] {
            assert!(proposals_query(bad).is_err(), "{bad:?}");
        }
        for bad in ["0x", "0x5FA4", "\") { id }", &DEMO_ID.to_uppercase()] {
            assert!(proposal_query(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn vote_type_supported_only_for_single_uint_choice_types() {
        assert!(vote_type_supported("single-choice"));
        assert!(vote_type_supported("basic"));
        for kind in ["approval", "ranked-choice", "quadratic", "weighted", "shutter", "", "Basic"] {
            assert!(!vote_type_supported(kind), "{kind}");
        }
    }

    #[test]
    fn voting_open_needs_active_state_and_now_before_end() {
        assert!(voting_open("active", 100, Some(99)));
        assert!(!voting_open("active", 100, Some(100)), "end is exclusive");
        assert!(!voting_open("active", 100, Some(101)));
        assert!(!voting_open("active", 100, None), "unknown time is never open");
        for state in ["closed", "pending", "", "Active"] {
            assert!(!voting_open(state, 100, Some(1)), "{state}");
        }
    }

    #[test]
    fn choices_are_numbered_from_one() {
        let choices = number_choices(&["For".into(), "Against".into(), "Abstain".into()]);
        assert_eq!(choices[0], Choice { number: 1, label: "For".into() });
        assert_eq!(choices[2], Choice { number: 3, label: "Abstain".into() });
        assert!(number_choices(&[]).is_empty());
    }

    #[test]
    fn testnet_listing_dedupes_and_orders_newest_first() {
        let data: ProposalsData =
            parse_response(200, recorded!("ballot_proposals_testnet.json"), MAX_PROPOSALS_RESPONSE_BYTES).unwrap();
        assert_eq!((data.active.len(), data.recent.len()), (3, 4));
        // 1_794_000_000 is before every active end in the recording.
        let doc = proposals_document(Network::Testnet, "bloom-ballot-182c.eth", data, Some(1_794_000_000));
        assert_eq!(doc["network"], "testnet");
        assert_eq!(doc["space"], "bloom-ballot-182c.eth");
        assert_eq!(doc["fetched_at"], 1_794_000_000_u64);
        let proposals = doc["proposals"].as_array().unwrap();
        let titles: Vec<&str> = proposals.iter().map(|p| p["title"].as_str().unwrap()).collect();
        assert_eq!(
            titles,
            ["Short-lived test", "Lower protocol fee to 0.2%", "Treasury transfer to grants multisig", "bloom-ballot test"]
        );
        let demo = &proposals[1];
        assert_eq!(demo["id"], DEMO_ID);
        assert_eq!(demo["type"], "single-choice");
        assert_eq!(demo["state"], "active");
        assert_eq!(demo["start"], 1_793_989_313_u64);
        assert_eq!(demo["vote_type_supported"], true);
        assert_eq!(demo["voting_open"], true);
        assert_eq!(demo["choices"], json!([
            {"number": 1, "label": "For"}, {"number": 2, "label": "Against"}, {"number": 3, "label": "Abstain"}
        ]));
        assert_eq!(proposals[0]["state"], "closed");
        assert_eq!(proposals[0]["id"], CLOSED_ID);
        assert_eq!(proposals[0]["vote_type_supported"], true);
        assert_eq!(proposals[0]["voting_open"], false);
        assert_eq!(proposals[3]["scores_total"], 1.0);
        assert_eq!(proposals[3]["voting_open"], true);
        let keys: Vec<&String> = demo.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 10, "{keys:?}");
        // After the active proposals end nothing is open; without a clock nothing is.
        let data: ProposalsData =
            parse_response(200, recorded!("ballot_proposals_testnet.json"), MAX_PROPOSALS_RESPONSE_BYTES).unwrap();
        let later = merge_proposals(data.active.clone(), data.recent.clone(), Some(1_794_594_112));
        assert!(later.iter().all(|p| !p.voting_open));
        let unknown = merge_proposals(data.active, data.recent, None);
        assert!(unknown.iter().all(|p| !p.voting_open));
    }

    #[test]
    fn mainnet_listing_projects_basic_proposals() {
        let data: ProposalsData =
            parse_response(200, recorded!("ballot_proposals_mainnet.json"), MAX_PROPOSALS_RESPONSE_BYTES).unwrap();
        let newest = data.recent[0].id.clone();
        let created: Vec<u64> = data.recent.iter().map(|p| p.created.unwrap()).collect();
        assert!(created.windows(2).all(|w| w[0] >= w[1]), "the hub orders recent by created desc");
        let merged = merge_proposals(data.active, data.recent, None);
        assert_eq!(merged.len(), 10, "the one active proposal is also among the 10 recent");
        assert_eq!(merged[0].id, newest);
        assert!(merged.iter().filter(|p| p.kind == "basic").all(|p| p.vote_type_supported));
    }

    #[test]
    fn step0_recordings_still_project() {
        // Older recordings use the unaliased `proposals` field and fewer fields.
        #[derive(Deserialize)]
        struct Old {
            proposals: Vec<HubSummary>,
        }
        let recent: Old = parse_response(200, recorded!("q_proposals_recent.json"), 1 << 20).unwrap();
        let active: Old = parse_response(200, recorded!("q_proposals_active.json"), 1 << 20).unwrap();
        assert!(active.proposals.is_empty());
        let mut doubled = recent.proposals.clone();
        doubled.extend(recent.proposals.clone());
        let merged = merge_proposals(doubled, recent.proposals, None);
        assert_eq!(merged.len(), 5);
        assert!(merged.windows(2).all(|w| w[0].start >= w[1].start), "newest first by start");
        assert_eq!(merged[0].kind, "basic");
        assert!(merged.iter().all(|p| p.vote_type_supported && p.choices.is_empty()));
    }

    #[test]
    fn testnet_proposal_projects_every_field() {
        let data: ProposalData =
            parse_response(200, recorded!("ballot_proposal_testnet.json"), MAX_PROPOSAL_RESPONSE_BYTES).unwrap();
        let detail = project_proposal(Network::Testnet, data.proposal.unwrap(), Some(1_794_000_000));
        assert_eq!(detail.id, DEMO_ID);
        assert_eq!(detail.space.as_deref(), Some("bloom-ballot-182c.eth"));
        assert_eq!(detail.author, "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596");
        assert_eq!(detail.title, "Lower protocol fee to 0.2%");
        assert!(!detail.truncated);
        assert_eq!(detail.choices.len(), 3);
        assert_eq!(detail.kind, "single-choice");
        // The hub's start (1 s after the signed 1793989312), never a local value.
        assert_eq!((detail.start, detail.end), (1_793_989_313, 1_794_594_112));
        assert_eq!(detail.snapshot, Some(json!(12_078_297)));
        assert_eq!(detail.state, "active");
        assert!(detail.scores.is_empty());
        assert_eq!(detail.scores_total, Some(0.0));
        assert_eq!(detail.votes, Some(0));
        assert!(detail.vote_type_supported);
        assert!(detail.voting_open);
        let doc = serde_json::to_value(&detail).unwrap();
        for key in ["network", "fetched_at", "id", "space", "author", "title", "body", "truncated", "choices",
                    "type", "start", "end", "snapshot", "state", "scores", "scores_total", "votes", "vote_type_supported", "voting_open"] {
            assert!(doc.get(key).is_some(), "missing {key}");
        }
        assert_eq!(doc["network"], "testnet");
    }

    #[test]
    fn long_mainnet_body_is_cut_at_8192_bytes() {
        let data: ProposalData =
            parse_response(200, recorded!("ballot_proposal_mainnet.json"), MAX_PROPOSAL_RESPONSE_BYTES).unwrap();
        let raw = data.proposal.unwrap();
        assert!(raw.body.len() > BODY_MAX_BYTES);
        let full = raw.body.clone();
        let detail = project_proposal(Network::Mainnet, raw, None);
        assert!(detail.truncated);
        assert!(detail.body.len() <= BODY_MAX_BYTES && detail.body.len() > BODY_MAX_BYTES - 4);
        assert!(full.starts_with(&detail.body));
        assert_eq!(detail.kind, "basic");
        assert!(detail.vote_type_supported);
        assert!(!detail.voting_open, "no clock was passed");
        assert_eq!(detail.scores.len(), 3);
        assert_eq!(detail.votes, Some(62));
    }

    #[test]
    fn multibyte_body_is_cut_on_a_character_boundary() {
        let data: ProposalData = parse_response(200, recorded!("q_proposal.json"), 1 << 20).unwrap();
        let mut raw = data.proposal.unwrap();
        assert!(!raw.body.is_ascii());
        let short = project_proposal(Network::Testnet, raw.clone(), None);
        assert!(!short.truncated, "a body under 8192 bytes is kept whole");
        assert_eq!(short.body, raw.body);
        assert_eq!(short.space.as_deref(), Some("cryptolootlegends.eth"));
        assert_eq!(short.votes, None, "step0 recording did not ask for votes");
        // Pad with 3-byte characters so the 8192 cut lands inside one.
        // 1 + 3 * 2730 = 8191: byte 8192 is the middle of a character.
        raw.body = format!("a{}", "提".repeat(4000));
        let cut = project_proposal(Network::Testnet, raw, None);
        assert!(cut.truncated);
        assert_eq!(cut.body.len(), 1 + 3 * 2730);
        assert!(cut.body.ends_with('提'));
        assert!(cut.body.len() <= BODY_MAX_BYTES);
    }

    #[test]
    fn unknown_proposal_decodes_as_none() {
        let data: ProposalData =
            parse_response(200, recorded!("ballot_proposal_testnet_null.json"), MAX_PROPOSAL_RESPONSE_BYTES).unwrap();
        assert!(data.proposal.is_none());
    }

    #[test]
    fn oversized_recorded_response_is_rejected() {
        let body = recorded!("ballot_proposal_mainnet.json");
        assert_eq!(parse_response::<ProposalData>(200, body, 1_024).unwrap_err(), HubError::TooLarge);
    }

    #[test]
    fn non_numeric_snapshot_is_dropped() {
        let raw: HubProposal = serde_json::from_value(json!({"id": DEMO_ID, "snapshot": {"x": 1}})).unwrap();
        assert_eq!(project_proposal(Network::Mainnet, raw, None).snapshot, None);
        let raw: HubProposal = serde_json::from_value(json!({"id": DEMO_ID, "snapshot": "123"})).unwrap();
        assert_eq!(project_proposal(Network::Mainnet, raw, None).snapshot, Some(json!("123")));
    }
}
