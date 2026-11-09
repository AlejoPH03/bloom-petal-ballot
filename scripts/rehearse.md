# Dress rehearsal runbook

For a human, on the final local build. Testnet only. It casts one real testnet
vote from the Bloom account on the rehearsal proposal, and tests that the
mandate refuses a blocked proposal.

The vote steps have **not** been run on this build: the expected output comes
from `docs/plan-votes.md` §12 (the live acceptance of the same flow) and the
route code. Stop at the first step that differs.

Do not vote on "Lower protocol fee to 0.2%": it is reserved for the demo. It
and the "Treasury transfer" proposal close 2026-11-13T18:21Z. The rehearsal
proposal closes 2026-11-16T14:35Z.

## 0. Build, enroll, restart

```sh
cd ~/work/bloom-petal-ballot
git status --short    # expect nothing
bash scripts/dev-cycle.sh
```

Type `ENROLL` when asked, restart the triad when asked, and change no file in
the repo until step 10 is done.

Expected last line of the build section:
`OK: enrolled == built == installed: <hash>`.

## 1. Helpers (paste once, in a new shell, after the restart)

```sh
cd ~/work/bloom-petal-ballot
source /tmp/bloom-triad-logs/triad.env
WALLET=main; A=/petals/ballot/testnet/accounts/$WALLET/0; V=$A/votes
SPACE=bloom-ballot-182c.eth
DELEGATE=0x042abe4EE65cBC71Bd11297DCea2d69e1650a596
REHEARSAL="Rehearsal: extend voting period to 5 days"
TREASURY=0x8c45d4c1f422023c56ba41b41d00f441df20b0a14ae22a0eab5e1eb77da505e1

# M: the marker of the last vote write (read-after-write)
M()  { bloom vfs cat "$V/new.json" | jq -c '.last_write | {outcome, code, vote_id, record, message}'; }
# ID: the pending ceremony id, taken from the marker
ID() { bloom vfs cat "$V/new.json" | jq -r '.last_write.message // ""' | sed -n 's/^approval pending (\([^)]*\)).*/\1/p'; }
# W: write $BODY to votes/new.json, then show the marker
W()  { bloom vfs write "$V/new.json" --data "$BODY"; M; }
# VID, R: the current vote's id and its record path
VID() { jq -r .vote_id <<<"$BODY"; }
R()  { echo "$V/$(VID).json"; }
# REC: the fields of the vote record that matter
REC() { bloom vfs cat "$(R)" | jq -c '{status, reason_code, digest, pending: .approval.pending.action_id, signature_sha256, receipt, counted, live: .live.counted, next}'; }
# URL: find the pending ceremony from the marker and print its URL
URL() {
  local id out; id="$(ID)"
  [ -n "$id" ] || { echo "no pending id in the last write; run M" >&2; return 1; }
  if out="$(bloom vfs cat "/wallets/$WALLET/policy-updates/pending/$id/approval_challenge.json" 2>/dev/null)"; then
    echo "POLICY UPDATE $id"
  elif out="$(bloom vfs cat "/petal-signing-requests/$id.json" 2>/dev/null)"; then
    echo "SIGNING REQUEST $id"
  else
    echo "no ceremony found for $id" >&2; return 1
  fi
  jq -r .ceremony_url <<<"$out"
}
# CER: how many ceremonies exist (compare before and after a refused vote)
CER() { { bloom vfs ls /petal-signing-requests; bloom vfs ls "/wallets/$WALLET/policy-updates/pending"; } 2>/dev/null | wc -l; }
```

## 2. Status

```sh
bloom vfs cat /petals/ballot/status.json | jq -c '{petal, version}'
```

Expected: `{"petal":"ballot","version":"0.1.0"}`

## 3. Set the mandate

```sh
bloom vfs write "$A/mandate.json" --data '{"spaces":["'$SPACE'"],"follow":{"'$SPACE'":"'$DELEGATE'"},"blocked_keywords":["treasury transfer"],"require_reason":true,"max_votes_per_day":5}'
bloom vfs cat "$A/mandate.json" | jq -c '{mandate, outcome: .last_write.outcome}'
```

Expected: the mandate back with `"require_reason":true`,
`"max_votes_per_day":5`, the blocked keyword, your space in `spaces`, the
delegate in `follow`, and `"outcome":"saved"`.

## 4. Read the inbox and following

```sh
bloom vfs cat "$A/inbox.json" | jq -c '.spaces[].open[] | {title, voted, our_choice}'
bloom vfs cat "$A/inbox.json" | jq -c '.alerts[] | {kind, title}'
bloom vfs cat "$A/following.json" | jq -c '.following[].proposals[] | {title, delegate_choice, vote_id: .suggested_write.body.vote_id}'
```

Expected:

- Inbox open: four proposals. `Rehearsal: extend voting period to 5 days`,
  `Lower protocol fee to 0.2%` and `Treasury transfer to grants multisig` have
  `voted:false`. `bloom-ballot test` has `voted:true` with
  `our_choice {"number":1,"label":"For"}`.
- Alerts: `delegate_voted` for the rehearsal and Lower-fee proposals. Before
  2026-11-12T18:21Z there is no `closing_soon`.
- Following: exactly the rehearsal and Lower-fee proposals, each with
  `delegate_choice {"number":1,"label":"For"}`. The rehearsal's `vote_id` is
  `follow-cba6d117`.

## 5. Write the suggested body, for the rehearsal only

```sh
BODY="$(bloom vfs cat "$A/following.json" | jq -c --arg t "$REHEARSAL" 'first(.following[].proposals[] | select(.title == $t)) | .suggested_write.body')"
echo "$BODY"
W
```

Expected body:
`{"choice":1,"proposal":"0xcba6d1177cba4dfa21e2377025145cdc9c0084e3f783be79085869cb1d5fb63b","reason":"Following 0x042abe4EE65cBC71Bd11297DCea2d69e1650a596","space":"bloom-ballot-182c.eth","vote_id":"follow-cba6d117"}`

Expected from `W`: an error ending in `approval pending (<id>); the owner opens
the ceremony from …`, and a marker with `"outcome":"approval_pending"`,
`"code":-3`, `"vote_id":"follow-cba6d117"`.

## 6. Policy ceremony (first signing attempt for this hash)

```sh
URL
REC
```

Expected: `POLICY UPDATE <id>` and a URL; `REC` shows
`"status":"awaiting_approval"` and `digest` `0x…`. Open the URL and approve
("Approve policy change", which adds this package hash).

## 7. Identical write: the signing request

```sh
W
URL
```

Expected: `approval pending (<a different id>)`, marker `approval_pending`, and
`SIGNING REQUEST <id>` with a URL. Writing `W` again before approving returns
the same id and opens no new ceremony.

## 8. Verify the digest with Bloom's own tool (before approving)

```sh
bloom vfs cat "$(R)" | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-votes/in.json
bloom vfs cat /tools/eip712/hash/ballot-votes/out.hex; echo
bloom vfs cat "$(R)" | jq -r .verify.expected_out_hex
bloom vfs cat "$(R)" | jq -c '.typed_data.message | {space, proposal, choice, reason, from}'
```

Expected: the two hex values are identical, and the message shows your space,
the rehearsal proposal, choice 1 and the reason. Bloom computes the first value
from the readable typed data, so this does not rely on Ballot's own hashing.
The ceremony page shows only a hash. Keep this value open for step 9.

## 9. Signing ceremony

Open the URL from step 7 and check that the hash under "The exact plan that
was reviewed" (`exact_ordered_hashes`, shown without `0x`) equals the value
from step 8. Approve, then go straight to step 10: Signer has refused approvals
about 90 s before their stated expiry.

## 10. Identical write: submitted

```sh
W
REC
```

Expected: exit 0, marker `"outcome":"submitted"`, `"code":0`. `REC` shows
`"status":"submitted"`, `receipt.id` equal to `digest`, and a
`signature_sha256`. The vote is on Snapshot now.

## 11. Identical write: counted

```sh
bloom vfs cat "$(R)" | jq -c .live
W
REC
```

Expected: `live` has `"checked":true`, `"counted":true` and
`hub_vote.id` equal to `digest`. After `W`, marker `"outcome":"counted"`, and
`REC` shows `"status":"counted"`. If `live.counted` is false, wait a few
seconds and repeat; it opens no ceremony.

## 12. The inbox shows it voted

```sh
bloom vfs cat "$A/inbox.json" | jq -c --arg t "$REHEARSAL" '.spaces[].open[] | select(.title == $t) | {title, voted, our_choice}'
bloom vfs cat "$A/following.json" | jq -c '[.following[].proposals[].title]'
```

Expected: `"voted":true` and `"our_choice":{"number":1,"label":"For"}`. The
rehearsal is no longer offered by `following.json`, only Lower-fee.

## 13. A blocked vote is refused with no ceremony

```sh
before="$(CER)"
BODY='{"vote_id":"rehearsal-treasury","space":"'$SPACE'","proposal":"'$TREASURY'","choice":1,"reason":"Should be refused by the mandate"}'
W
REC
echo "ceremonies before=$before after=$(CER)"
```

Expected: `W` fails with `refused: mandate_blocked_keyword: the proposal's
title contains the blocked keyword "treasury transfer"`; marker `"outcome":
"refused"`, `"code":-3`; `REC` shows `"status":"refused"`,
`"reason_code":"mandate_blocked_keyword"`, and no digest or `pending`. The
ceremony counts match.

## If something differs

- **`signing refused: missing installer provenance, …`** (step 6 or 7): the
  installed hash is not the enrolled one, or the triad was not restarted after
  enrollment. Rerun step 0. Machine's `petal.sign_payload_denied` log line has
  the real reason. Do not edit `policy.json`.
- **`approval_expired` or `expired`:** the vote is final. Set a new `vote_id`
  in `$BODY` (for example with `jq -c '.vote_id="rehearsal-2"'`) and repeat
  from step 5. It opens a new ceremony.
- **`CEREMONY_RATE_LIMITED`:** finish or let the live ceremony expire, then
  retry the same write.
- **A rebuild between steps** loses the in-flight vote (known-limitations §8).
  Rerun from step 0.
