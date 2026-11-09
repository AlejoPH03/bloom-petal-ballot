# Demo script (3 minutes)

The recording casts one real testnet vote from the Bloom account on "Lower
protocol fee to 0.2%", using the `follow-5fa4f371` suggested body. Adapted from
`scripts/rehearse.md`, whose flow was run live on 2026-11-09
(`docs/evidence/`).

That proposal and "Treasury transfer to grants multisig" close
**2026-11-13T18:21Z**. After 2026-11-12T18:21Z the inbox also shows
`closing_soon` alerts, so check the expected output in shot 2.

The vote can be cast only once with this `vote_id`. A failed take needs a new
`vote_id` (see "If a take fails").

## Pre-recording checklist

**Off camera, in this order:**

- [ ] Docs are final and `git status --short` prints nothing. Any file change, including a docs change, is a new package hash.
- [ ] Triad is up (`docs/dev-setup-wsl.md`).
- [ ] Fresh build: `bash scripts/dev-cycle.sh`, ending in `OK: enrolled == built == installed: <hash>`. A new hash is what makes the policy ceremony of shot 3 appear. Do not change a file between this step and the end of the recording.
- [ ] Paste the helpers below in a new terminal, after the triad restart.
- [ ] Write the mandate. A new build starts with an empty store, so it is gone otherwise:

  ```sh
  bloom vfs write "$A/mandate.json" --data '{"spaces":["'$SPACE'"],"follow":{"'$SPACE'":"'$DELEGATE'"},"blocked_keywords":["treasury transfer"],"require_reason":true,"max_votes_per_day":5}'
  bloom vfs cat "$A/mandate.json" | jq -c '{outcome: .last_write.outcome}'
  ```

  Expected: `{"outcome":"saved"}`.

- [ ] Dry-read shot 2's commands. `following.json` must offer Lower-fee.
- [ ] Passkey is ready, and no other ceremony is pending.
- [ ] Open testnet.snapshot.box in a tab, go to space `bloom-ballot-182c.eth` and open "Lower protocol fee to 0.2%".

**Screen:**

- [ ] Terminal font 18 pt or larger, about 100 columns, plain prompt, history cleared.
- [ ] Browser zoom 125%.
- [ ] **Ceremony URLs never on screen in full.** Capture the browser below its address bar, or crop it. Use `CU` below, which copies the link and does not print it. Do not paste it anywhere visible.
- [ ] Notifications off, and nothing private on the desktop.

## Helpers (off camera)

From the repository root:

```sh
source /tmp/bloom-triad-logs/triad.env
WALLET=main; A=/petals/ballot/testnet/accounts/$WALLET/0; V=$A/votes
SPACE=bloom-ballot-182c.eth
DELEGATE=0x042abe4EE65cBC71Bd11297DCea2d69e1650a596
DEMO="Lower protocol fee to 0.2%"
TREASURY=0x8c45d4c1f422023c56ba41b41d00f441df20b0a14ae22a0eab5e1eb77da505e1

M()  { bloom vfs cat "$V/new.json" | jq -c '.last_write | {outcome, code, vote_id, record, message}'; }
ID() { bloom vfs cat "$V/new.json" | jq -r '.last_write.message // ""' | sed -n 's/^approval pending (\([^)]*\)).*/\1/p'; }
W()  { bloom vfs write "$V/new.json" --data "$BODY"; M; }
VID() { jq -r .vote_id <<<"$BODY"; }
R()  { echo "$V/$(VID).json"; }
REC() { bloom vfs cat "$(R)" | jq -c '{status, reason_code, digest, signature_sha256, receipt_id: .receipt.id, counted: .counted.hub_vote_id, live: .live.counted}'; }
CER() { { bloom vfs ls /petal-signing-requests; bloom vfs ls "/wallets/$WALLET/policy-updates/pending"; } 2>/dev/null | wc -l; }
# CU: copy the pending ceremony link to the Windows clipboard, never print it
CU() {
  local id out kind; id="$(ID)"
  [ -n "$id" ] || { echo "no pending id; run M" >&2; return 1; }
  if out="$(bloom vfs cat "/wallets/$WALLET/policy-updates/pending/$id/approval_challenge.json" 2>/dev/null)"; then kind="POLICY UPDATE"
  elif out="$(bloom vfs cat "/petal-signing-requests/$id.json" 2>/dev/null)"; then kind="SIGNING REQUEST"
  else echo "no ceremony found for $id" >&2; return 1; fi
  jq -r .ceremony_url <<<"$out" | clip.exe
  echo "$kind ${id:0:12}… link copied"
}
```

## Shot list

| # | Shot | Time | Length |
| --- | --- | --- | --- |
| 1 | Problem | 0:00 | 15 s |
| 2 | The agent's view: status, inbox, following | 0:15 | 30 s |
| 3 | First write: policy ceremony (default-deny) | 0:45 | 25 s |
| 4 | Second write: verify the digest with Bloom's tool | 1:10 | 25 s |
| 5 | Signing ceremony | 1:35 | 20 s |
| 6 | Submitted | 1:55 | 10 s |
| 7 | Counted, and visible on Snapshot | 2:05 | 20 s |
| 8 | Mandate refusal, no ceremony | 2:25 | 20 s |
| 9 | Closing | 2:45 | 15 s |

### 1. Problem (0:00, 15 s)

**Screen:** title card "ballot, a Bloom Petal", then the proposal open on Snapshot.

**Voice:** "Most DAO holders don't vote, because it takes time. An agent could
help, but giving it a key means it can sign anything. Bloom refuses to sign
arbitrary typed data, so ballot makes one signature, a Snapshot vote, a named
operation the owner approves."

### 2. The agent's view (0:15, 30 s)

```sh
bloom vfs cat /petals/ballot/status.json | jq -c '{petal, version}'
bloom vfs cat "$A/inbox.json" | jq -c '.spaces[].open[]? | {title, voted}'
bloom vfs cat "$A/inbox.json" | jq -c '.alerts[] | {kind, title}'
bloom vfs cat "$A/following.json" | jq -c '.following[].proposals[]? | {title, delegate_choice, vote_id: .suggested_write.body.vote_id}'
```

**Screen:** the four results. Expected: four open proposals, with Lower-fee and
Treasury `voted:false`; a `delegate_voted` alert for Lower-fee; `following`
offers Lower-fee, For, `follow-5fa4f371`.

**Voice:** "The agent starts from its inbox. It sees what's open, and that the
delegate this account follows voted on the fee proposal. Ballot offers a ready
vote body. Nothing votes automatically."

### 3. First write: policy ceremony (0:45, 25 s)

```sh
BODY="$(bloom vfs cat "$A/following.json" | jq -c --arg t "$DEMO" 'first(.following[].proposals[]? | select(.title == $t)) | .suggested_write.body')"
echo "$BODY" | jq -c '{vote_id, choice, reason}'
W
CU
```

**Screen:** the error ending in `approval pending (<id>)`, marker
`"outcome":"approval_pending"`, then `POLICY UPDATE … link copied`. Paste into
the browser with the address bar out of frame, and approve with the passkey.

**Voice:** "The write doesn't sign anything. Bloom is default-deny: this build's
package hash isn't in the wallet's policy, so the owner has to add it first."

### 4. Verify the digest (1:10, 25 s)

```sh
W
CU
bloom vfs cat "$(R)" | jq -c '.typed_data.message | {space, proposal, choice, reason}'
bloom vfs cat "$(R)" | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-votes/in.json
bloom vfs cat /tools/eip712/hash/ballot-votes/out.hex; echo
bloom vfs cat "$(R)" | jq -r .verify.expected_out_hex
```

**Screen:** `SIGNING REQUEST … link copied`, the readable vote, then two
identical hex lines. Highlight them.

**Voice:** "Now the exact vote is pending. At these Bloom versions the ceremony
shows only a hash, so we don't trust ballot's hash. Bloom's own tool recomputes
it from the readable vote."

### 5. Signing ceremony (1:35, 20 s)

**Screen:** paste the link with the address bar out of frame. Show "The exact
plan that was reviewed", put the hash next to the terminal's value, and approve.
Go straight to shot 6, because Signer has refused approvals about 90 s early.

**Voice:** "The same hash is on the ceremony page. The owner approves this one
vote with a passkey."

### 6. Submitted (1:55, 10 s)

```sh
W
REC
```

**Screen:** marker `"outcome":"submitted"`; `REC` has `status:"submitted"` and
`receipt_id` equal to `digest`.

**Voice:** "Same write again. Signed and submitted. Snapshot's receipt is that
same digest."

### 7. Counted, visible on Snapshot (2:05, 20 s)

```sh
W
REC
```

If `live` is false, wait a few seconds and repeat.

**Screen:** `status:"counted"` and `counted` equal to `digest`. Then reload the
Snapshot tab and show the new vote from `0x0c05…2751` with its reason.

**Voice:** "Counted only means the hub shows a vote with that id and choice. And
here it is on Snapshot."

### 8. Mandate refusal (2:25, 20 s)

```sh
before="$(CER)"
BODY='{"vote_id":"demo-treasury","space":"'$SPACE'","proposal":"'$TREASURY'","choice":1,"reason":"Should be refused by the mandate"}'
W
REC
echo "ceremonies before=$before after=$(CER)"
```

**Screen:** `refused: mandate_blocked_keyword: …"treasury transfer"`,
`status:"refused"`, no digest, equal ceremony counts.

**Voice:** "The mandate filters bad drafts before the owner is ever asked. No
binding, no ceremony. It's advisory, so the real boundary is still the owner's
approval."

### 9. Closing (2:45, 15 s)

**Screen:** the README's digest table, or the route tree.

**Voice:** "One protocol signature, turned into a named, reviewable operation.
The agent does the reading. The owner keeps the authority. And no funds moved."

## If a take fails

- **Before the signing ceremony:** restart from shot 3. The same body resumes the same vote.
- **After `approval_expired` or `expired`:** the `vote_id` is final. In shot 3, add `BODY="$(jq -c '.vote_id="demo-2"' <<<"$BODY")"`.
- **After `submitted`:** the vote is on Snapshot. Re-record the remaining shots, since re-voting would change the record.
- **`signing refused: missing installer provenance`:** the installed hash is not the enrolled one, or the triad was not restarted. Rerun `dev-cycle.sh`.
