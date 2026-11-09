//! `following.json`: open proposals where a delegate the owner follows has
//! voted and this account has not, each with a ready `votes/new.json` body.
//!
//! Per `follow` entry, at most three hub calls, the inbox's recorded queries
//! (FINDINGS.md §11-12): the space's proposals, the delegate's votes on the
//! open ones, this account's votes on those the delegate voted on. The read
//! never writes the store and never votes.

use serde_json::{Value, json};

use crate::Network;
use crate::hub::HubError;
use crate::inbox::{InboxHost, delegate_voted, followable, label, votes_by_proposal};
use crate::mandate::{self, FOLLOWING_MAX_ENTRIES, Mandate};
use crate::votes::store::Scope;

pub const FOLLOWING_SCHEMA: &str = "ballot.following.v1";

/// The `votes/new.json` write that copies the delegate's vote.
/// `proposal` is a validated id (`delegate_voted`), so the slice holds.
fn suggested_write(scope: &Scope, space: &str, delegate: &str, proposal: &str, choice: u32) -> Value {
    json!({
        "path": format!("{}/new.json", scope.votes_dir()),
        "body": {
            "vote_id": format!("follow-{}", &proposal[2..10]),
            "space": space,
            "proposal": proposal,
            "choice": choice,
            "reason": format!("Following {delegate}"),
        },
    })
}

struct Entry<'a> {
    space: &'a str,
    delegate: &'a str,
    address: &'a str,
    now: Option<u64>,
}

fn proposals_to_follow<H: InboxHost>(host: &mut H, scope: &Scope, entry: &Entry) -> Result<Vec<Value>, HubError> {
    let network = scope.network;
    let data = host.hub_inbox(network, entry.space)?;
    let open = followable(&data, entry.now);
    if open.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = open.iter().map(|p| p.id.clone()).collect();
    let delegate_votes = host.hub_voted(network, entry.delegate, &ids)?;
    let voted_by_delegate = delegate_voted(&open, &delegate_votes);
    if voted_by_delegate.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = voted_by_delegate.iter().map(|(p, _)| p.id.clone()).collect();
    let ours = votes_by_proposal(&host.hub_voted(network, entry.address, &ids)?);
    Ok(voted_by_delegate
        .into_iter()
        .filter(|(p, _)| !ours.contains_key(&p.id.to_ascii_lowercase()))
        .map(|(p, choice)| {
            json!({
                "id": p.id,
                "title": p.title,
                "closes_in_s": entry.now.map(|now| p.end.saturating_sub(now)),
                "delegate_choice": label(&p.choices, Some(choice)),
                "suggested_write": suggested_write(scope, entry.space, entry.delegate, &p.id, choice),
            })
        })
        .collect())
}

fn notes(mandate: &Mandate, network: Network, now: Option<u64>) -> Vec<String> {
    let mut notes = vec![
        "Every vote still needs the owner's approval in a passkey ceremony: writing a suggested_write body only starts that request.".to_owned(),
        "The mandate still applies when the body is written (allowed spaces, blocked keywords, require_reason, daily limit), so a body shown here can be refused.".to_owned(),
        "Listed: open proposals of a supported type that the delegate voted on and this account has not. Nothing here votes automatically.".to_owned(),
        "vote_id is derived from the proposal: if that vote_id already ended (expired, rejected) or the delegate changed its choice, write under a new vote_id.".to_owned(),
    ];
    if mandate.follow.len() > FOLLOWING_MAX_ENTRIES {
        notes.push(format!("only the first {FOLLOWING_MAX_ENTRIES} of {} follow entries are shown", mandate.follow.len()));
    }
    if now.is_none() {
        notes.push("clock unavailable: no proposal is shown as open".into());
    }
    if network == Network::Mainnet {
        notes.push("votes on mainnet are disabled in this build: a suggested_write here is refused as mainnet_disabled".into());
    }
    notes
}

/// The `following.json` read.
pub fn following_document<H: InboxHost>(host: &mut H, scope: &Scope) -> Result<Value, String> {
    let now = host.now_ms().filter(|&ms| ms > 0).map(|ms| ms / 1000);
    let address = host.account_address()?;
    let mandate = mandate::load(host, scope)?;
    let mut doc = json!({
        "schema": FOLLOWING_SCHEMA,
        "network": scope.network.as_str(),
        "account": {"wallet": scope.wallet, "index": scope.index, "address": address},
        "fetched_at": now,
        "mandate_updated_ms": mandate.as_ref().map(|m| m.updated_ms),
        "following": [],
        "advisory": mandate::ADVISORY,
    });
    let Some(mandate) = mandate.filter(|m| !m.follow.is_empty()) else {
        doc["explanation"] = json!(format!(
            "No delegates to follow: set them in the mandate, e.g. bloom vfs write {}/mandate.json --data '{{\"follow\":{{\"<space>.eth\":\"0x<delegate address>\"}}}}' (keep any other mandate fields you use; a write replaces the whole mandate).",
            scope.account_dir()
        ));
        return Ok(doc);
    };
    let mut following = Vec::new();
    for (space, delegate) in mandate.follow.iter().take(FOLLOWING_MAX_ENTRIES) {
        let entry = Entry { space, delegate, address: &address, now };
        let mut item = match proposals_to_follow(host, scope, &entry) {
            Ok(proposals) => json!({"space": space, "delegate": delegate, "proposals": proposals}),
            Err(error) => json!({"space": space, "delegate": delegate, "error": error.message(scope.network)}),
        };
        if let Err((code, detail)) = mandate.check_space(space) {
            item["warning"] = json!(format!("{code}: {detail}; a vote here would be refused"));
        }
        following.push(item);
    }
    doc["following"] = json!(following);
    doc["notes"] = json!(notes(&mandate, scope.network, now));
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::tests::{BLOOM, DELEGATE, FakeInbox, LOWER_FEE, NOW, fake, scope, testnet_delegate_voted, testnet_inbox};
    use crate::inbox::{InboxData, InboxProposal, VotedRef, VotedVote};
    use crate::votes::body::VoteBody;

    const SPACE: &str = "bloom-ballot-182c.eth";

    fn set_mandate(host: &mut FakeInbox, body: Value) {
        let (code, message) = mandate::write(host, &scope(), Some(1), body.to_string().as_bytes());
        assert_eq!(code, 0, "{message}");
    }

    fn following_testnet() -> FakeInbox {
        let mut host = fake();
        set_mandate(&mut host, json!({"spaces": [SPACE], "follow": {SPACE: DELEGATE}}));
        host.inbox.insert(SPACE.into(), Ok(testnet_inbox()));
        host
    }

    fn open(id: &str, kind: &str) -> InboxProposal {
        InboxProposal {
            id: id.into(),
            title: format!("open {id}"),
            kind: kind.into(),
            choices: vec!["For".into(), "Against".into()],
            end: NOW + 3600,
            state: "active".into(),
            scores: vec![],
        }
    }

    fn vote(id: &str, choice: Value) -> VotedVote {
        VotedVote { choice, proposal: Some(VotedRef { id: id.into() }) }
    }

    #[test]
    fn recorded_delegate_vote_offers_only_the_lower_fee_proposal_with_the_exact_body() {
        let mut host = following_testnet();
        let writes_before = host.writes;
        let doc = following_document(&mut host, &scope()).unwrap();
        let entry = &doc["following"][0];
        assert_eq!(entry["delegate"], DELEGATE);
        let proposals = entry["proposals"].as_array().unwrap();
        // "bloom-ballot test": this account voted. Treasury: the delegate did not.
        assert_eq!(proposals.len(), 1);
        let p = &proposals[0];
        assert_eq!(p["id"], LOWER_FEE);
        assert_eq!(p["title"], "Lower protocol fee to 0.2%");
        assert_eq!(p["closes_in_s"], 1_794_594_112 - NOW);
        assert_eq!(p["delegate_choice"], json!({"number": 1, "label": "For"}));
        assert_eq!(p["suggested_write"]["path"], "/petals/ballot/testnet/accounts/main/0/votes/new.json");
        let body = &p["suggested_write"]["body"];
        assert_eq!(
            body,
            &json!({
                "vote_id": "follow-5fa4f371",
                "space": SPACE,
                "proposal": LOWER_FEE,
                "choice": 1,
                "reason": format!("Following {DELEGATE}"),
            })
        );
        let parsed = VoteBody::parse(body.to_string().as_bytes()).expect("the body votes/new.json accepts");
        assert_eq!(parsed.reason, format!("Following {DELEGATE}"));
        let notes = doc["notes"].to_string();
        assert!(notes.contains("owner's approval") && notes.contains("mandate still applies"));
        assert!(entry.get("warning").is_none());
        assert_eq!(host.calls, [format!("inbox {SPACE}"), format!("voted {DELEGATE} 3"), format!("voted {BLOOM} 2")]);
        assert_eq!(host.writes, writes_before, "the read writes nothing");
    }

    #[test]
    fn without_follow_entries_it_explains_how_to_set_them() {
        let mut host = fake();
        for _ in 0..2 {
            let doc = following_document(&mut host, &scope()).unwrap();
            let explanation = doc["explanation"].as_str().unwrap();
            assert!(explanation.contains("mandate.json") && explanation.contains("\"follow\""));
            assert_eq!(doc["following"], json!([]));
            assert_eq!(doc["account"]["address"], BLOOM);
            set_mandate(&mut host, json!({"spaces": ["a.eth"]}));
        }
        assert!(host.calls.is_empty());
    }

    #[test]
    fn entries_render_independently_and_bounds_hold() {
        let mut host = fake();
        let follow: serde_json::Map<String, Value> = (0..12).map(|i| (format!("s{i:02}.eth"), json!(DELEGATE))).collect();
        set_mandate(&mut host, json!({"follow": follow}));
        host.inbox.insert("s00.eth".into(), Err(HubError::Status(502)));
        host.inbox.insert("s01.eth".into(), Ok(testnet_inbox()));
        let doc = following_document(&mut host, &scope()).unwrap();
        let entries = doc["following"].as_array().unwrap();
        assert_eq!(entries.len(), FOLLOWING_MAX_ENTRIES);
        assert!(entries[0]["error"].as_str().unwrap().contains("502"));
        assert_eq!(entries[1]["proposals"].as_array().unwrap().len(), 1);
        assert_eq!(entries[2]["proposals"], json!([]));
        assert!(doc["notes"].to_string().contains("first 10 of 12"));
        // One entry with open proposals: 3 calls; the others: 1 each.
        assert_eq!(host.calls.len(), FOLLOWING_MAX_ENTRIES + 2);
    }

    #[test]
    fn a_failed_votes_query_fails_only_that_entry() {
        let mut host = following_testnet();
        host.delegate = Err(HubError::Decode);
        let doc = following_document(&mut host, &scope()).unwrap();
        assert!(doc["following"][0]["error"].is_string());
        let mut host = following_testnet();
        host.voted = Err(HubError::Status(503));
        let doc = following_document(&mut host, &scope()).unwrap();
        assert!(doc["following"][0]["error"].as_str().unwrap().contains("503"));
        assert!(doc["following"][0].get("proposals").is_none());
    }

    #[test]
    fn only_open_supported_proposals_with_a_usable_delegate_choice_are_offered() {
        let hex = |c: char| format!("0x{}", c.to_string().repeat(64));
        let (good, ranked, out_of_range, not_a_number) = (hex('a'), hex('b'), hex('c'), hex('d'));
        let mut ended = open(&hex('e'), "single-choice");
        ended.end = NOW;
        let data = InboxData {
            open: vec![
                open(&good, "basic"),
                open(&ranked, "ranked-choice"),
                open(&out_of_range, "single-choice"),
                open(&not_a_number, "single-choice"),
                ended,
                open("QmLegacyIpfsId", "single-choice"),
            ],
            recent: vec![],
        };
        let mut host = fake();
        set_mandate(&mut host, json!({"follow": {SPACE: DELEGATE}}));
        host.inbox.insert(SPACE.into(), Ok(data));
        host.voted = Ok(vec![]);
        host.delegate = Ok(vec![
            vote(&good, json!(2)),
            vote(&ranked, json!([1, 2])),
            vote(&out_of_range, json!(3)),
            vote(&not_a_number, json!({"1": 1})),
            vote(&hex('e'), json!(1)),
            vote("QmLegacyIpfsId", json!(1)),
        ]);
        let doc = following_document(&mut host, &scope()).unwrap();
        let proposals = doc["following"][0]["proposals"].as_array().unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0]["delegate_choice"], json!({"number": 2, "label": "Against"}));
        assert_eq!(proposals[0]["suggested_write"]["body"]["vote_id"], "follow-aaaaaaaa");
    }

    #[test]
    fn no_delegate_vote_skips_the_account_query() {
        let mut host = following_testnet();
        host.delegate = Ok(vec![]);
        let doc = following_document(&mut host, &scope()).unwrap();
        assert_eq!(doc["following"][0]["proposals"], json!([]));
        assert_eq!(host.calls.len(), 2);
    }

    #[test]
    fn a_followed_space_outside_the_mandate_spaces_is_flagged() {
        let mut host = fake();
        set_mandate(&mut host, json!({"spaces": ["other.eth"], "follow": {SPACE: DELEGATE}}));
        host.inbox.insert(SPACE.into(), Ok(testnet_inbox()));
        let doc = following_document(&mut host, &scope()).unwrap();
        assert!(doc["following"][0]["warning"].as_str().unwrap().contains("mandate_space_not_allowed"));
        assert_eq!(doc["following"][0]["proposals"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn no_clock_shows_nothing_open_and_mainnet_says_votes_are_refused() {
        let mut host = following_testnet();
        host.now = None;
        let doc = following_document(&mut host, &scope()).unwrap();
        assert_eq!(doc["following"][0]["proposals"], json!([]));
        assert!(doc["notes"].to_string().contains("clock unavailable"));
        let mainnet = Scope::new(Network::Mainnet, "main", 0);
        let mut host = fake();
        let (code, _) = mandate::write(&mut host, &mainnet, Some(1), json!({"follow": {SPACE: DELEGATE}}).to_string().as_bytes());
        assert_eq!(code, 0);
        let doc = following_document(&mut host, &mainnet).unwrap();
        assert!(doc["notes"].to_string().contains("mainnet_disabled"));
    }

    #[test]
    fn recorded_delegate_fixture_has_the_demo_vote() {
        let votes = testnet_delegate_voted();
        let by_id = votes_by_proposal(&votes);
        assert_eq!(by_id.get(LOWER_FEE), Some(&Some(1)));
        assert_eq!(by_id.len(), 2, "its Step 0 vote and the demo vote");
    }
}
