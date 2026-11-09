# Demo script (3 minutes)

The presenter talks in plain language to a separate Claude Code session, the
"governance agent" (`docs/agent-setup/CLAUDE.md`), which operates ballot through
the Bloom filesystem. Ceremonies stay in the browser. The recording casts one real
testnet vote from the Bloom account on "Lower protocol fee to 0.2%".

The underlying flow was run live on 2026-11-09 with direct commands
(`docs/evidence/`). **The agent-driven version has not been run yet**: use the
agent rehearsal below first. Timings are for the edited cut, with the agent's
thinking pauses trimmed.

| Proposal | Use | Closes |
| --- | --- | --- |
| "Agent rehearsal: adopt monthly community calls" | Agent rehearsal | 2026-11-13T16:13Z |
| "Lower protocol fee to 0.2%" | The recording. One vote only | 2026-11-13T18:21Z |
| "Treasury transfer to grants multisig" | Mandate refusal | 2026-11-13T18:21Z |

After 2026-11-12T18:21Z the inbox also shows `closing_soon` alerts.

## Agent rehearsal

Run shots 2 to 8 once off camera, saying "the monthly community calls proposal"
instead of "the fee proposal". The delegate has voted For on it
(`docs/snapshot-testnet/FINDINGS.md` §14), and the suggested `vote_id` should be
`follow-e5c498f4`. Check that the agent follows its limits: it shows the vote
before asking, keeps links out of the transcript, and does not say counted early.

**The recording needs a build the wallet's policy has not seen**, or shot 3 opens
no policy ceremony. A rebuild of unchanged files keeps the same hash. So after
the rehearsal, make a real change (for example, commit the rehearsal's evidence)
and run `dev-cycle.sh` again. Appendix A4 checks this.

## Pre-recording checklist

- [ ] Docs are final and `git status --short` prints nothing. Any file change, including a docs change, is a new package hash.
- [ ] Triad is up (`docs/dev-setup-wsl.md`).
- [ ] Fresh build: `bash scripts/dev-cycle.sh`, ending in `OK: enrolled == built == installed: <hash>`. Appendix A4 shows the hash is not yet in the policy. Change no file until the recording ends.
- [ ] Start the agent in its own directory (Appendix A1), in a fresh session.
- [ ] Mandate written, because a new build starts with an empty store. Tell the agent: "Set my mandate: only the space bloom-ballot-182c.eth, follow delegate 0x042abe4EE65cBC71Bd11297DCea2d69e1650a596 there, block the keyword 'treasury transfer', require a reason, at most 5 votes a day." Or use Appendix A2.
- [ ] Allow the agent's read commands, so no permission dialog interrupts the take.
- [ ] Passkey is ready, and no other ceremony is pending.
- [ ] testnet.snapshot.box open at space `bloom-ballot-182c.eth`, on "Lower protocol fee to 0.2%".
- [ ] Agent terminal font 18 pt or larger, about 100 columns. Browser zoom 125%.
- [ ] **Ceremony URLs never on screen in full.** The agent puts them on the clipboard. Capture the browser below its address bar, or crop it.
- [ ] Notifications off, and nothing private on the desktop.

## Evidence capture during the recording

Save to `~/work/evidence/`. Commands are in Appendix A5.

- [ ] **After shot 4, before approving:** Bloom's verify output, as `demo-verify-out.hex`. The next verify overwrites it.
- [ ] **Shot 5, before approving:** a screenshot of the ceremony page's exact hash, URL blurred, as `ceremony-exact-hash.png`.
- [ ] **Shot 7:** a screenshot of the vote on testnet.snapshot.box, as `snapshot-vote.png`.
- [ ] **After shot 8:** the vote record and the refused record, as JSON.
- [ ] **Before adding any of it to the repo:** check that no image shows a ceremony URL, and run `bash scripts/leak-check.sh ~/work/evidence`.

## Shot list

| # | Time | Length | Spoken prompt |
| --- | --- | --- | --- |
| 1 | 0:00 | 15 s | None (title card) |
| 2 | 0:15 | 30 s | "What needs my attention in governance?" |
| 3 | 0:45 | 25 s | "Vote with my delegate on the fee proposal: For, using the suggested reason." |
| 4 | 1:10 | 25 s | "Approved." |
| 5 | 1:35 | 20 s | "What should I check on this page?" |
| 6 | 1:55 | 10 s | "Approved." |
| 7 | 2:05 | 20 s | "Is it counted?" |
| 8 | 2:25 | 20 s | "Also vote For on the treasury transfer, reason 'fund the grants'." |
| 9 | 2:45 | 15 s | None (closing line) |

### 1. Problem (0:00, 15 s)

- **Prompt:** none.
- **Agent:** idle.
- **On screen:** title card "ballot, a Bloom Petal", then the proposal on Snapshot.
- **Voice-over:** "Most DAO holders don't vote, because it takes time. An agent could help, but giving it a key means it can sign anything. Bloom refuses to sign arbitrary typed data, so ballot makes one signature, a Snapshot vote, a named operation the owner approves."

### 2. What needs my attention (0:15, 30 s)

- **Prompt:** "What needs my attention in governance?"
- **Agent does:** reads `/petals/ballot/AGENTS.md`, `inbox.json` and `following.json`.
- **Agent says:** which proposals are open and unvoted, and that the delegate voted For on "Lower protocol fee to 0.2%".
- **On screen:** the agent's terminal.
- **Voice-over:** "The agent works from files Bloom serves. It starts from an inbox: what's open, and what the delegate I follow has done. Nothing votes automatically."

### 3. Vote: policy ceremony (0:45, 25 s)

- **Prompt:** "Vote with my delegate on the fee proposal: For, using the suggested reason."
- **Agent does:** writes the suggested body unchanged (`follow-5fa4f371`), reads the marker and the record, copies the link to the clipboard.
- **Agent says:** the title, For, the reason and the digest; that this is a policy update adding this build to the wallet's policy; and to open the link in the browser.
- **On screen:** the agent's answer, then the browser with the address bar out of frame. Approve with the passkey.
- **Voice-over:** "The agent can ask, but it can't sign. Bloom is default-deny: this build isn't in the wallet's policy, so I have to add it first."

### 4. The agent verifies the digest (1:10, 25 s)

- **Prompt:** "Approved."
- **Agent does:** repeats the identical write, reads the marker and the record, runs the record's two `verify.commands`, copies the signing link.
- **Agent says:** the title, For, the reason, Bloom's digest and that it matches; and to compare it on the ceremony page.
- **On screen:** the agent's answer, with the digest highlighted.
- **Voice-over:** "Now this exact vote is pending. At these Bloom versions the ceremony shows only a hash, so the agent has Bloom's own tool recompute it from the readable vote."

### 5. Signing ceremony (1:35, 20 s)

- **Prompt:** "What should I check on this page?"
- **Agent does:** nothing new.
- **Agent says:** compare the digest with the hash under "The exact plan that was reviewed", and approve only if they match.
- **On screen:** the browser with the address bar out of frame, the hash beside the agent's value. Approve, and go straight to shot 6, because Signer has refused approvals about 90 s early.
- **Voice-over:** "The same hash is on the ceremony page. I approve this one vote with a passkey."

### 6. Submitted (1:55, 10 s)

- **Prompt:** "Approved."
- **Agent does:** immediately repeats the identical write, then reads the marker and the record.
- **Agent says:** submitted, Snapshot's receipt equals the digest, not counted yet.
- **On screen:** the agent's answer.
- **Voice-over:** "Signed and submitted. Snapshot's receipt is that same digest."

### 7. Counted, visible on Snapshot (2:05, 20 s)

- **Prompt:** "Is it counted?"
- **Agent does:** checks `live.counted`, repeats the identical write once it is true, reads the marker and the record.
- **Agent says:** counted, only if the record says so.
- **On screen:** the agent's answer, then the Snapshot tab reloaded, showing the new vote from `0x0c05…2751`.
- **Voice-over:** "It only says counted when the hub shows a vote with that id. And here it is on Snapshot."

### 8. Mandate refusal (2:25, 20 s)

- **Prompt:** "Also vote For on the treasury transfer, reason 'fund the grants'."
- **Agent does:** writes the vote, reads the marker and the record. Does not touch the mandate.
- **Agent says:** refused by the mandate (blocked keyword "treasury transfer"), nothing bound, no ceremony opened.
- **On screen:** the agent's answer. The browser stays idle.
- **Voice-over:** "The mandate filters bad drafts before I'm ever asked. It's advisory, so the real boundary is still my approval."

### 9. Closing (2:45, 15 s)

- **Prompt:** none.
- **Agent:** idle.
- **On screen:** the README's digest table, or the route tree.
- **Voice-over:** "One protocol signature, turned into a named, reviewable operation. The agent does the reading. The owner keeps the authority. And no funds moved."

## If a take fails

- **The agent paraphrases, or misreads a status:** re-record that answer. The vote's state is in the record, so nothing is lost.
- **Before the signing ceremony:** restart from shot 3. The same body resumes the same vote.
- **After `approval_expired` or `expired`:** the `vote_id` is final. Ask the agent to write the same vote under a new `vote_id`, such as `demo-2`.
- **After `submitted`:** the vote is on Snapshot. Re-record the remaining shots, since re-voting would change the record.
- **`signing refused: missing installer provenance`:** the installed hash is not the enrolled one, or the triad was not restarted. Rerun `dev-cycle.sh`.
- **The agent stalls on camera:** use Appendix A3 from a second terminal.

## Appendix: commands (fallback)

### A1. Agent setup

From the repository root:

```sh
mkdir -p ~/work/ballot-agent
cp docs/agent-setup/CLAUDE.md ~/work/ballot-agent/CLAUDE.md
cd ~/work/ballot-agent && claude
```

### A2. Helpers and mandate

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

bloom vfs write "$A/mandate.json" --data '{"spaces":["'$SPACE'"],"follow":{"'$SPACE'":"'$DELEGATE'"},"blocked_keywords":["treasury transfer"],"require_reason":true,"max_votes_per_day":5}'
bloom vfs cat "$A/mandate.json" | jq -c '{outcome: .last_write.outcome}'   # expect {"outcome":"saved"}
```

### A3. Each shot by hand

```sh
# Shot 2
bloom vfs cat "$A/inbox.json" | jq -c '.spaces[].open[]? | {title, voted}'
bloom vfs cat "$A/inbox.json" | jq -c '.alerts[] | {kind, title}'
bloom vfs cat "$A/following.json" | jq -c '.following[].proposals[]? | {title, delegate_choice, vote_id: .suggested_write.body.vote_id}'

# Shot 3: expect "approval pending (<id>)", then POLICY UPDATE
BODY="$(bloom vfs cat "$A/following.json" | jq -c --arg t "$DEMO" 'first(.following[].proposals[]? | select(.title == $t)) | .suggested_write.body')"
W
CU

# Shot 4: expect SIGNING REQUEST, then two identical hex lines
W
CU
bloom vfs cat "$(R)" | jq -c '.typed_data.message | {space, proposal, choice, reason}'
bloom vfs cat "$(R)" | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-votes/in.json
bloom vfs cat /tools/eip712/hash/ballot-votes/out.hex; echo
bloom vfs cat "$(R)" | jq -r .verify.expected_out_hex

# Shot 6: expect status submitted, receipt_id equal to digest
W
REC

# Shot 7: if live is false, wait a few seconds and repeat. Expect status counted
W
REC

# Shot 8: expect mandate_blocked_keyword and equal ceremony counts
before="$(CER)"
BODY='{"vote_id":"demo-treasury","space":"'$SPACE'","proposal":"'$TREASURY'","choice":1,"reason":"fund the grants"}'
W
REC
echo "ceremonies before=$before after=$(CER)"
```

### A4. Is this build new to the wallet's policy?

```sh
source /tmp/bloom-triad-logs/triad.env
HASH="$(bloom petals build . | sed -n 's/^hash: //p')"
bloom vfs cat /wallets/main/policy.json | jq --arg h "$HASH" '.allowed_petal_packages | index($h) == null'
```

`true` means shot 3 will open a policy ceremony.

### A5. Evidence capture

```sh
source /tmp/bloom-triad-logs/triad.env
A=/petals/ballot/testnet/accounts/main/0; V=$A/votes
# After shot 4
bloom vfs cat /tools/eip712/hash/ballot-votes/out.hex > ~/work/evidence/demo-verify-out.hex
# After shot 8. Use the vote_id the agent used, if it differs
bloom vfs cat "$V/follow-5fa4f371.json" > ~/work/evidence/demo-follow-vote.json
REFUSED="$(bloom vfs cat "$V/new.json" | jq -r .last_write.record)"
bloom vfs cat "$A/$REFUSED" > ~/work/evidence/demo-refused.json
```
