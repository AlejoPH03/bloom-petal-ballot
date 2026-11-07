//! The agent's starting point: open proposals in the mandate's spaces, with
//! whether this account voted, and alerts (docs/plan-mandate-inbox.md).
//!
//! Per space, at most two hub calls (bound three): `INBOX_QUERY` (open and
//! recent proposals with scores) and `VOTED_QUERY` (this account's votes on
//! those proposals via `proposal_in`). Both texts are recorded in
//! `docs/snapshot-testnet/responses/ballot_{inbox,voted}.graphql`
//! (FINDINGS.md §11). The read never writes the store.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Network;
use crate::hub::{HubError, HubQuery};
use crate::mandate::{self, INBOX_MAX_SPACES};
use crate::proposals::{vote_type_supported, voting_open};
use crate::votes::store::{KvStore, Scope};

pub const INBOX_QUERY: &str = "query Inbox($space: String!) { open: proposals(first: 20, where: {space: $space, state: \"active\"}, orderBy: \"end\", orderDirection: asc) { id title type choices start end state scores scores_total } recent: proposals(first: 10, where: {space: $space}, orderBy: \"created\", orderDirection: desc) { id title type choices start end state scores scores_total } }";
pub const VOTED_QUERY: &str = "query Voted($voter: String!, $proposals: [String]!) { votes(first: 50, where: {voter: $voter, proposal_in: $proposals}) { id choice created proposal { id } } }";

pub const MAX_INBOX_RESPONSE_BYTES: usize = 256 * 1024;
pub const MAX_VOTED_RESPONSE_BYTES: usize = 64 * 1024;
/// An open proposal closing sooner than this, not yet voted, is an alert.
pub const CLOSING_SOON_SECS: u64 = 24 * 60 * 60;

pub fn inbox_query(space: &str) -> HubQuery {
    HubQuery::new(INBOX_QUERY, json!({"space": space}), MAX_INBOX_RESPONSE_BYTES)
}

pub fn voted_query(voter: &str, proposals: &[String]) -> HubQuery {
    HubQuery::new(VOTED_QUERY, json!({"voter": voter, "proposals": proposals}), MAX_VOTED_RESPONSE_BYTES)
}

#[derive(Clone, Debug, Deserialize)]
pub struct InboxProposal {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub end: u64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub scores: Vec<f64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct InboxData {
    #[serde(default)]
    pub open: Vec<InboxProposal>,
    #[serde(default)]
    pub recent: Vec<InboxProposal>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VotedRef {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VotedVote {
    #[serde(default)]
    pub choice: Value,
    #[serde(default)]
    pub proposal: Option<VotedRef>,
}

#[derive(Debug, Deserialize)]
pub struct VotedData {
    #[serde(default)]
    pub votes: Vec<VotedVote>,
}

/// What the inbox needs from the host.
pub trait InboxHost: KvStore {
    fn now_ms(&mut self) -> Option<u64>;
    fn account_address(&mut self) -> Result<String, String>;
    fn hub_inbox(&mut self, network: Network, space: &str) -> Result<InboxData, HubError>;
    fn hub_voted(&mut self, network: Network, voter: &str, proposals: &[String]) -> Result<Vec<VotedVote>, HubError>;
}

fn label(choices: &[String], number: Option<u32>) -> Value {
    match number {
        Some(n) if n >= 1 && (n as usize) <= choices.len() => json!({"number": n, "label": choices[n as usize - 1]}),
        Some(n) => json!({"number": n, "label": null}),
        None => Value::Null,
    }
}

/// The winning choice numbers (1-based) by score; empty without scores.
pub fn winners(scores: &[f64]) -> Vec<u32> {
    let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if scores.is_empty() || max <= 0.0 {
        return Vec::new();
    }
    (1..).zip(scores).filter(|(_, score)| **score == max).map(|(n, _)| n).collect()
}

/// One space's entry and its alerts, from the hub data and this account's votes.
pub fn project_space(space: &str, data: &InboxData, votes: &[VotedVote], now: Option<u64>) -> (Value, Vec<Value>) {
    let voted: BTreeMap<String, Option<u32>> = votes
        .iter()
        .filter_map(|vote| {
            let id = vote.proposal.as_ref()?.id.to_ascii_lowercase();
            Some((id, vote.choice.as_u64().and_then(|n| u32::try_from(n).ok())))
        })
        .collect();
    let mut alerts = Vec::new();
    let mut open = Vec::new();
    for proposal in data.open.iter().filter(|p| voting_open(&p.state, p.end, now)) {
        let ours = voted.get(&proposal.id.to_ascii_lowercase());
        let closes_in_s = now.map(|now| proposal.end.saturating_sub(now));
        if ours.is_none() && closes_in_s.is_some_and(|secs| secs < CLOSING_SOON_SECS) {
            alerts.push(json!({
                "kind": "closing_soon", "space": space, "proposal": proposal.id, "title": proposal.title,
                "closes_in_s": closes_in_s,
            }));
        }
        open.push(json!({
            "id": proposal.id,
            "title": proposal.title,
            "type": proposal.kind,
            "vote_type_supported": vote_type_supported(&proposal.kind),
            "closes_in_s": closes_in_s,
            "end": proposal.end,
            "voted": ours.is_some(),
            "our_choice": ours.map(|choice| label(&proposal.choices, *choice)).unwrap_or(Value::Null),
        }));
    }
    for proposal in data.recent.iter().filter(|p| p.state == "closed") {
        let Some(ours) = voted.get(&proposal.id.to_ascii_lowercase()) else { continue };
        let win = winners(&proposal.scores);
        let won = match (ours, win.is_empty()) {
            (_, true) | (None, _) => Value::Null,
            (Some(ours), false) => json!(win.contains(ours)),
        };
        alerts.push(json!({
            "kind": "result", "space": space, "proposal": proposal.id, "title": proposal.title,
            "winning_choice": win.first().map(|n| label(&proposal.choices, Some(*n))).unwrap_or(Value::Null),
            "tie": win.len() > 1,
            "winning_choices": win.iter().map(|n| label(&proposal.choices, Some(*n))).collect::<Vec<_>>(),
            "our_choice": label(&proposal.choices, *ours),
            "our_choice_won": won,
        }));
    }
    (json!({"space": space, "open": open}), alerts)
}

/// Proposal ids to ask `VOTED_QUERY` about: the open ones and the recent
/// closed ones, each once.
pub fn voted_ids(data: &InboxData) -> Vec<String> {
    let mut ids: Vec<String> = data
        .open
        .iter()
        .chain(data.recent.iter().filter(|p| p.state == "closed"))
        .map(|p| p.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The `inbox.json` read.
pub fn inbox_document<H: InboxHost>(host: &mut H, scope: &Scope) -> Result<Value, String> {
    let now_ms = host.now_ms().filter(|&ms| ms > 0);
    let now = now_ms.map(|ms| ms / 1000);
    let address = host.account_address()?;
    let mandate = mandate::load(host, scope)?;
    let mut doc = json!({
        "schema": "ballot.inbox.v1",
        "network": scope.network.as_str(),
        "account": {"wallet": scope.wallet, "index": scope.index, "address": address},
        "fetched_at": now,
        "mandate_updated_ms": mandate.as_ref().map(|m| m.updated_ms),
        "spaces": [],
        "alerts": [],
        "advisory": mandate::ADVISORY,
    });
    let spaces: Vec<String> = mandate.map(|m| m.spaces).unwrap_or_default();
    if spaces.is_empty() {
        doc["explanation"] = json!(format!(
            "No spaces to watch: set them in the mandate, e.g. bloom vfs write {}/mandate.json --data '{{\"spaces\":[\"<space>.eth\"]}}' (keep any other mandate fields you use; a write replaces the whole mandate).",
            scope.account_dir()
        ));
        return Ok(doc);
    }
    let mut entries = Vec::new();
    let mut alerts = Vec::new();
    let mut notes = vec![
        "voted and our_choice come from the hub: votes by this account's address".to_owned(),
        format!("closing_soon: open, not voted, closes in under {} h; result: closed proposals among the space's 10 most recent that this account voted on", CLOSING_SOON_SECS / 3600),
    ];
    if spaces.len() > INBOX_MAX_SPACES {
        notes.push(format!("only the first {INBOX_MAX_SPACES} of {} mandate spaces are shown", spaces.len()));
    }
    if now.is_none() {
        notes.push("clock unavailable: no proposal is shown as open".into());
    }
    let network = scope.network;
    for space in spaces.iter().take(INBOX_MAX_SPACES) {
        let data = match host.hub_inbox(network, space) {
            Ok(data) => data,
            Err(error) => {
                entries.push(json!({"space": space, "error": error.message(network)}));
                continue;
            }
        };
        let ids = voted_ids(&data);
        let votes = if ids.is_empty() {
            Vec::new()
        } else {
            match host.hub_voted(network, &address, &ids) {
                Ok(votes) => votes,
                Err(error) => {
                    entries.push(json!({"space": space, "error": error.message(network)}));
                    continue;
                }
            }
        };
        let (entry, space_alerts) = project_space(space, &data, &votes, now);
        entries.push(entry);
        alerts.extend(space_alerts);
    }
    doc["spaces"] = json!(entries);
    doc["alerts"] = json!(alerts);
    doc["notes"] = json!(notes);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::parse_response;
    use crate::votes::store::fake::MemStore;
    use crate::votes::store::{DelIfValue, PutNew};

    macro_rules! recorded {
        ($name:literal) => {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/snapshot-testnet/responses/", $name))
        };
    }

    const BLOOM: &str = "0x0c056ae661D3d3f87919eCb9733527509D392751";
    const TEST_P: &str = "0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00";
    /// 2026-11-08, inside every open proposal's window.
    const NOW: u64 = 1_794_100_000;

    fn testnet_inbox() -> InboxData {
        parse_response(200, recorded!("ballot_inbox_testnet.json"), MAX_INBOX_RESPONSE_BYTES).unwrap()
    }

    fn testnet_voted() -> Vec<VotedVote> {
        let data: VotedData = parse_response(200, recorded!("ballot_voted_testnet.json"), MAX_VOTED_RESPONSE_BYTES).unwrap();
        data.votes
    }

    #[test]
    fn query_texts_are_the_recorded_ones() {
        assert_eq!(INBOX_QUERY.as_bytes(), recorded!("ballot_inbox.graphql"));
        assert_eq!(VOTED_QUERY.as_bytes(), recorded!("ballot_voted.graphql"));
        let q = voted_query(BLOOM, &[TEST_P.to_owned()]);
        assert_eq!(q.variables(), &json!({"voter": BLOOM, "proposals": [TEST_P]}));
        assert_eq!(inbox_query("a.eth").variables(), &json!({"space": "a.eth"}));
    }

    #[test]
    fn recorded_space_projects_open_proposals_and_our_vote() {
        let data = testnet_inbox();
        assert_eq!((data.open.len(), data.recent.len()), (3, 4));
        let (entry, alerts) = project_space("bloom-ballot-182c.eth", &data, &testnet_voted(), Some(NOW));
        let open = entry["open"].as_array().unwrap();
        let titles: Vec<&str> = open.iter().map(|p| p["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["bloom-ballot test", "Treasury transfer to grants multisig", "Lower protocol fee to 0.2%"], "by end, soonest first");
        assert_eq!(open[0]["voted"], true, "the accepted live vote");
        assert_eq!(open[0]["our_choice"], json!({"number": 1, "label": "For"}));
        assert_eq!(open[0]["closes_in_s"], 1_794_593_689 - NOW);
        assert_eq!(open[0]["vote_type_supported"], true);
        assert_eq!(open[1]["voted"], false);
        assert!(alerts.is_empty(), "nothing closes within 24 h; the closed one was not voted on");
        assert_eq!(voted_ids(&data).len(), 4, "3 open + the closed recent one");
    }

    #[test]
    fn closing_soon_only_for_open_unvoted_proposals_under_a_day() {
        let data = testnet_inbox();
        // 1 s inside a day before the latest end (1794594112): both unvoted ones are near.
        let near_end = 1_794_594_112 - CLOSING_SOON_SECS + 1;
        let (_, alerts) = project_space("s.eth", &data, &testnet_voted(), Some(near_end));
        let kinds: Vec<(&str, &str)> = alerts.iter().map(|a| (a["kind"].as_str().unwrap(), a["title"].as_str().unwrap())).collect();
        assert_eq!(kinds, [("closing_soon", "Treasury transfer to grants multisig"), ("closing_soon", "Lower protocol fee to 0.2%")]);
        let (entry, alerts) = project_space("s.eth", &data, &testnet_voted(), Some(1_794_593_689));
        assert!(entry["open"].as_array().unwrap().iter().all(|p| p["title"] != "bloom-ballot test"), "ended: not open");
        assert_eq!(alerts.len(), 2);
        let (entry, _) = project_space("s.eth", &data, &[], None);
        assert!(entry["open"].as_array().unwrap().is_empty(), "no clock: nothing open");
    }

    fn closed(id: &str, scores: Vec<f64>) -> InboxProposal {
        InboxProposal {
            id: id.into(),
            title: format!("closed {id}"),
            kind: "single-choice".into(),
            choices: vec!["For".into(), "Against".into(), "Abstain".into()],
            end: 1,
            state: "closed".into(),
            scores,
        }
    }

    fn vote(id: &str, choice: u32) -> VotedVote {
        VotedVote { choice: json!(choice), proposal: Some(VotedRef { id: id.into() }) }
    }

    #[test]
    fn results_name_the_winner_our_choice_and_whether_it_won() {
        let data = InboxData {
            open: vec![],
            recent: vec![
                closed("0xa", vec![3.0, 1.0, 0.0]),
                closed("0xb", vec![1.0, 3.0, 0.0]),
                closed("0xc", vec![2.0, 2.0, 0.0]),
                closed("0xd", vec![]),
                closed("0xe", vec![5.0, 0.0, 0.0]),
            ],
        };
        let votes = [vote("0xA", 1), vote("0xb", 1), vote("0xc", 2), vote("0xd", 3)];
        let (_, alerts) = project_space("s.eth", &data, &votes, Some(NOW));
        assert_eq!(alerts.len(), 4, "0xe was not voted on");
        assert_eq!(alerts[0]["winning_choice"]["label"], "For");
        assert_eq!(alerts[0]["our_choice"]["label"], "For");
        assert_eq!(alerts[0]["our_choice_won"], true, "ids compare case-insensitively");
        assert_eq!(alerts[1]["winning_choice"]["label"], "Against");
        assert_eq!(alerts[1]["our_choice_won"], false);
        assert_eq!(alerts[2]["tie"], true);
        assert_eq!(alerts[2]["winning_choices"].as_array().unwrap().len(), 2);
        assert_eq!(alerts[2]["our_choice_won"], true);
        assert_eq!(alerts[3]["winning_choice"], Value::Null);
        assert_eq!(alerts[3]["our_choice_won"], Value::Null, "no scores, no verdict");
        assert_eq!(winners(&[0.0, 0.0]), Vec::<u32>::new());
    }

    struct FakeInbox {
        store: MemStore,
        inbox: BTreeMap<String, Result<InboxData, HubError>>,
        voted: Result<Vec<VotedVote>, HubError>,
        calls: Vec<String>,
    }

    impl KvStore for FakeInbox {
        fn store_get(&mut self, key: &str, max: usize) -> Result<Option<Vec<u8>>, String> { self.store.store_get(key, max) }
        fn store_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> { self.store.store_put(key, value) }
        fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<PutNew, String> { self.store.store_put_new(key, value) }
        fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<DelIfValue, String> { self.store.store_del_if_value(key, expected) }
        fn store_del(&mut self, key: &str) -> Result<(), String> { self.store.store_del(key) }
        fn store_list(&mut self, prefix: &str, max: usize) -> Result<Vec<String>, String> { self.store.store_list(prefix, max) }
        fn secret_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> { self.store.secret_put(key, value) }
        fn secret_get(&mut self, key: &str, max: usize) -> Result<Option<Vec<u8>>, String> { self.store.secret_get(key, max) }
        fn secret_del(&mut self, key: &str) -> Result<(), String> { self.store.secret_del(key) }
    }

    impl InboxHost for FakeInbox {
        fn now_ms(&mut self) -> Option<u64> { Some(NOW * 1000) }
        fn account_address(&mut self) -> Result<String, String> { Ok(BLOOM.into()) }
        fn hub_inbox(&mut self, _network: Network, space: &str) -> Result<InboxData, HubError> {
            self.calls.push(format!("inbox {space}"));
            self.inbox.get(space).cloned().unwrap_or_else(|| Ok(InboxData::default()))
        }
        fn hub_voted(&mut self, _network: Network, voter: &str, proposals: &[String]) -> Result<Vec<VotedVote>, HubError> {
            assert_eq!(voter, BLOOM);
            self.calls.push(format!("voted {}", proposals.len()));
            self.voted.clone()
        }
    }

    fn fake() -> FakeInbox {
        FakeInbox { store: MemStore::default(), inbox: BTreeMap::new(), voted: Ok(testnet_voted()), calls: Vec::new() }
    }

    fn scope() -> Scope {
        Scope::new(Network::Testnet, "main", 0)
    }

    #[test]
    fn without_mandate_spaces_the_inbox_explains_what_to_do() {
        let mut host = fake();
        let doc = inbox_document(&mut host, &scope()).unwrap();
        assert!(doc["explanation"].as_str().unwrap().contains("mandate.json"));
        assert_eq!(doc["spaces"], json!([]));
        assert_eq!(doc["account"]["address"], BLOOM);
        assert!(host.calls.is_empty());
        mandate::write(&mut host, &scope(), Some(1), br#"{"require_reason":true}"#);
        let doc = inbox_document(&mut host, &scope()).unwrap();
        assert!(doc.get("explanation").is_some(), "a mandate without spaces explains too");
    }

    #[test]
    fn spaces_render_independently_and_bounds_hold() {
        let mut host = fake();
        let spaces: Vec<String> = (0..12).map(|i| format!("s{i}.eth")).collect();
        let body = json!({"spaces": spaces}).to_string();
        mandate::write(&mut host, &scope(), Some(1), body.as_bytes());
        host.inbox.insert("s0.eth".into(), Ok(testnet_inbox()));
        host.inbox.insert("s1.eth".into(), Err(HubError::Status(502)));
        let doc = inbox_document(&mut host, &scope()).unwrap();
        let entries = doc["spaces"].as_array().unwrap();
        assert_eq!(entries.len(), INBOX_MAX_SPACES);
        assert_eq!(entries[0]["open"][0]["voted"], true);
        assert!(entries[1]["error"].as_str().unwrap().contains("502"));
        assert!(entries[2]["open"].as_array().unwrap().is_empty());
        // s0: inbox + voted; others: inbox only (no ids to ask about).
        assert_eq!(host.calls.iter().filter(|c| c.starts_with("voted")).count(), 1);
        assert_eq!(host.calls.len(), INBOX_MAX_SPACES + 1);
        assert!(doc["notes"].to_string().contains("first 10 of 12"));
        assert_eq!(doc["fetched_at"], NOW);
        host.voted = Err(HubError::Decode);
        let doc = inbox_document(&mut host, &scope()).unwrap();
        assert!(doc["spaces"][0]["error"].is_string(), "a failed votes query fails only that space");
        assert!(doc["spaces"][2]["open"].is_array());
    }
}
