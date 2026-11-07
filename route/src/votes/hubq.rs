//! Hub queries of the vote flow: voting power and this voter's vote on a
//! proposal. Query texts are the recorded ones
//! (`docs/snapshot-testnet/responses/ballot_{vp,votes}.graphql`, FINDINGS.md §10);
//! values travel only as GraphQL variables. The hub matches `voter` whatever
//! the case of its hex digits (FINDINGS.md §10); we always send the
//! checksummed address we sign as `from`.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::hub::HubQuery;

pub const VP_QUERY: &str = "query Vp($voter: String!, $space: String!, $proposal: String) { vp(voter: $voter, space: $space, proposal: $proposal) { vp vp_by_strategy vp_state } }";
pub const VOTES_QUERY: &str = "query Votes($proposal: String!, $voter: String!) { votes(first: 10, where: {proposal: $proposal, voter: $voter}) { id voter choice created } }";

pub const MAX_VP_RESPONSE_BYTES: usize = 16 * 1024;
pub const MAX_VOTES_RESPONSE_BYTES: usize = 64 * 1024;

/// Voting-power query. Inputs are validated by the caller (`VoteBody`, the
/// checksummed account address).
pub fn vp_query(voter: &str, space: &str, proposal: &str) -> HubQuery {
    HubQuery::new(
        VP_QUERY,
        json!({"voter": voter, "space": space, "proposal": proposal}),
        MAX_VP_RESPONSE_BYTES,
    )
}

/// This voter's votes on one proposal (at most one per voter on the hub,
/// snapshot-sequencer `writer/vote.ts:139-192`).
pub fn votes_query(proposal: &str, voter: &str) -> HubQuery {
    HubQuery::new(VOTES_QUERY, json!({"proposal": proposal, "voter": voter}), MAX_VOTES_RESPONSE_BYTES)
}

#[derive(Debug, Deserialize)]
pub struct VpData {
    pub vp: Option<VpValue>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VpValue {
    #[serde(default)]
    pub vp: f64,
    #[serde(default)]
    pub vp_state: String,
}

#[derive(Debug, Deserialize)]
pub struct VotesData {
    #[serde(default)]
    pub votes: Vec<HubVote>,
}

/// One vote as the hub returns it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct HubVote {
    pub id: String,
    #[serde(default)]
    pub voter: String,
    /// A number for single-choice/basic; other types use arrays or objects.
    #[serde(default)]
    pub choice: Value,
    #[serde(default)]
    pub created: u64,
}

impl HubVote {
    pub fn choice_number(&self) -> Option<u32> {
        self.choice.as_u64().and_then(|n| u32::try_from(n).ok())
    }
}

/// What the hub shows for our vote. `counted` needs the hub's vote id to equal
/// our digest (Snapshot's vote id is the EIP-712 digest, FINDINGS.md §10) and
/// the same choice.
#[derive(Clone, Debug, PartialEq)]
pub struct HubEvidence {
    pub counted: bool,
    /// The hub's vote for this voter on this proposal, if any.
    pub hub_vote: Option<HubVote>,
    /// A different vote by this voter that is newer than ours (the
    /// `superseded` hook).
    pub superseded_hint: bool,
}

pub fn evidence(votes: &[HubVote], digest: &str, choice: u32, timestamp: u64) -> HubEvidence {
    let ours = votes.iter().find(|vote| vote.id.eq_ignore_ascii_case(digest));
    match ours {
        Some(vote) => HubEvidence {
            counted: vote.choice_number() == Some(choice),
            hub_vote: Some(vote.clone()),
            superseded_hint: false,
        },
        None => {
            let other = votes.iter().max_by_key(|vote| vote.created).cloned();
            HubEvidence {
                counted: false,
                superseded_hint: other.as_ref().is_some_and(|vote| vote.created >= timestamp),
                hub_vote: other,
            }
        }
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

    const PROPOSAL: &str = "0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00";
    const BLOOM: &str = "0x0c056ae661D3d3f87919eCb9733527509D392751";
    const THROWAWAY: &str = "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596";
    const LIVE_ID: &str = "0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657";

    #[test]
    fn query_texts_are_the_recorded_ones() {
        assert_eq!(VP_QUERY.as_bytes(), recorded!("ballot_vp.graphql"));
        assert_eq!(VOTES_QUERY.as_bytes(), recorded!("ballot_votes.graphql"));
        let vp = vp_query(BLOOM, "bloom-ballot-182c.eth", PROPOSAL);
        assert_eq!(vp.variables(), &json!({"voter": BLOOM, "space": "bloom-ballot-182c.eth", "proposal": PROPOSAL}));
        let votes = votes_query(PROPOSAL, BLOOM);
        assert_eq!(votes.variables(), &json!({"proposal": PROPOSAL, "voter": BLOOM}));
        assert!(!String::from_utf8(votes.body()).unwrap().contains("0x0c056ae661D3d3f87919eCb9733527509D392751 "));
    }

    #[test]
    fn recorded_vp_decodes() {
        let data: VpData = parse_response(200, recorded!("ballot_vp_testnet.json"), MAX_VP_RESPONSE_BYTES).unwrap();
        let vp = data.vp.unwrap();
        assert_eq!(vp.vp, 1.0);
        assert_eq!(vp.vp_state, "final");
        let missing = parse_response::<VpData>(200, recorded!("ballot_vp_unknown_proposal.json"), MAX_VP_RESPONSE_BYTES);
        assert_eq!(missing.unwrap_err(), HubError::GraphQl("proposal not found".into()));
    }

    #[test]
    fn recorded_votes_give_counted_evidence_only_for_the_matching_digest() {
        let data: VotesData = parse_response(200, recorded!("ballot_votes_testnet.json"), MAX_VOTES_RESPONSE_BYTES).unwrap();
        assert_eq!(data.votes.len(), 1);
        assert_eq!(data.votes[0].voter, THROWAWAY);
        let hit = evidence(&data.votes, LIVE_ID, 1, 1_793_988_894);
        assert!(hit.counted);
        assert_eq!(hit.hub_vote.as_ref().unwrap().id, LIVE_ID);
        assert!(!hit.superseded_hint);
        // Same id, different choice: not counted (cannot happen on the hub,
        // but the rule requires both).
        assert!(!evidence(&data.votes, LIVE_ID, 2, 1_793_988_894).counted);
        // Another digest: the hub's vote is someone else's later vote.
        let other = evidence(&data.votes, &format!("0x{}", "1".repeat(64)), 1, 1_793_988_000);
        assert!(!other.counted);
        assert!(other.superseded_hint);
        let older = evidence(&data.votes, &format!("0x{}", "1".repeat(64)), 1, 1_793_999_999);
        assert!(!older.superseded_hint, "an older hub vote does not supersede ours");
        let lower: VotesData =
            parse_response(200, recorded!("ballot_votes_testnet_lowercase_voter.json"), MAX_VOTES_RESPONSE_BYTES).unwrap();
        assert_eq!(lower.votes, data.votes, "voter filter ignores hex case");
        let empty: VotesData =
            parse_response(200, recorded!("ballot_votes_testnet_empty.json"), MAX_VOTES_RESPONSE_BYTES).unwrap();
        let none = evidence(&empty.votes, LIVE_ID, 1, 0);
        assert_eq!(none, HubEvidence { counted: false, hub_vote: None, superseded_hint: false });
    }

    #[test]
    fn step0_votes_recording_decodes() {
        let data: VotesData = parse_response(200, recorded!("final_hub_check.json"), 1 << 20).unwrap();
        assert_eq!(data.votes[0].choice_number(), Some(1));
    }
}
