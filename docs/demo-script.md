# Demo script (3 minutes)

The presenter talks in plain language to a separate Claude Code session, the
"governance agent", which operates `/petals/ballot`. The recording casts one real
testnet vote from the Bloom account on "Lower protocol fee to 0.2%", using the
`follow-5fa4f371` suggested body.

The underlying flow was run live on 2026-11-09 with direct commands
(`docs/evidence/`). **The agent-driven version has not been run yet**, so rehearse
the reads off camera first. Timings are for the edited cut, with the agent's
thinking pauses trimmed.

That proposal and "Treasury transfer to grants multisig" close
**2026-11-13T18:21Z**. After 2026-11-12T18:21Z the inbox also shows
`closing_soon` alerts. The vote can be cast only once under this `vote_id`.

## Pre-recording checklist

**Off camera, in this order:**

- [ ] Docs are final and `git status --short` prints nothing. Any file change, including a docs change, is a new package hash.
- [ ] Triad is up (`docs/dev-setup-wsl.md`).
- [ ] Fresh build: `bash scripts/dev-cycle.sh`, ending in `OK: enrolled == built == installed: <hash>`. A new hash is what makes the policy ceremony of shot 3 appear, and it serves the current `AGENTS.md` to the agent. Change no file between this step and the end of the recording.
- [ ] Set up the agent (Appendix A1) and start it in its own directory.
- [ ] Write the mandate. A new build starts with an empty store. Tell the agent: "Set my mandate: only the space bloom-ballot-182c.eth, follow delegate 0x042abe4EE65cBC71Bd11297DCea2d69e1650a596 there, block the keyword 'treasury transfer', require a reason, at most 5 votes a day." Or use Appendix A2.
- [ ] Dry run of shot 2 only, then allow the agent's read commands so no permission dialog interrupts the take. Do not let it write a vote. Start a fresh agent session for the take.
- [ ] Passkey is ready, and no other ceremony is pending.
- [ ] Open testnet.snapshot.box in a tab, go to space `bloom-ballot-182c.eth` and open "Lower protocol fee to 0.2%".

**Screen:**

- [ ] Agent terminal font 18 pt or larger, about 100 columns.
- [ ] Browser zoom 125%.
- [ ] **Ceremony URLs never on screen in full.** The agent puts them on the clipboard and does not print them. Capture the browser below its address bar, or crop it.
- [ ] Notifications off, and nothing private on the desktop.

## Shot list

| # | Shot | Time | Length |
| --- | --- | --- | --- |
| 1 | Problem | 0:00 | 15 s |
| 2 | "What needs my attention?" | 0:15 | 30 s |
| 3 | "Follow my delegate": policy ceremony | 0:45 | 25 s |
| 4 | "Approved": the agent verifies the digest | 1:10 | 25 s |
| 5 | Signing ceremony | 1:35 | 20 s |
| 6 | "Approved": submitted | 1:55 | 10 s |
| 7 | "Is it counted?", and visible on Snapshot | 2:05 | 20 s |
| 8 | "Vote on the treasury transfer": refused | 2:25 | 20 s |
| 9 | Closing | 2:45 | 15 s |

### 1. Problem (0:00, 15 s)

**Screen:** title card "ballot, a Bloom Petal", then the proposal on Snapshot.

**Voice-over:** "Most DAO holders don't vote, because it takes time. An agent
could help, but giving it a key means it can sign anything. Bloom refuses to sign
arbitrary typed data, so ballot makes one signature, a Snapshot vote, a named
operation the owner approves."

### 2. What needs my attention (0:15, 30 s)

**Say to the agent:** "What needs my attention in governance?"

**The agent should:** read `AGENTS.md`, `inbox.json` and `following.json`, and
answer in a few lines: four open proposals, two already voted; the delegate voted
For on "Lower protocol fee to 0.2%" and you have not.

**Voice-over:** "The agent starts from an inbox: what's open, and what the
delegate I follow has done. Nothing votes automatically."

### 3. Follow the delegate: policy ceremony (0:45, 25 s)

**Say to the agent:** "Follow my delegate on the fee proposal."

**The agent should:** write the suggested body unchanged (`follow-5fa4f371`, For),
read the marker and record, report that approval is pending, say this is a policy
update adding this build's package hash, and put the link on the clipboard.

**Presenter:** paste the link with the address bar out of frame, and approve with
the passkey.

**Voice-over:** "The agent can ask, but it can't sign. Bloom is default-deny: this
build isn't in the wallet's policy, so I have to add it first."

### 4. The agent verifies the digest (1:10, 25 s)

**Say to the agent:** "Approved."

**The agent should:** repeat the identical write, get the signing request, run the
record's two `verify.commands`, and show the vote (space, title, For, reason) with
Bloom's `out.hex`, saying it equals the expected digest. It copies the signing link.

**Voice-over:** "Now this exact vote is pending. At these Bloom versions the
ceremony shows only a hash, so the agent has Bloom's own tool recompute it from
the readable vote."

### 5. Signing ceremony (1:35, 20 s)

**Presenter:** paste the link with the address bar out of frame. Show "The exact
plan that was reviewed", place the hash beside the agent's value, and approve. Go
straight to shot 6, because Signer has refused approvals about 90 s early.

**Voice-over:** "The same hash is on the ceremony page. I approve this one vote
with a passkey."

### 6. Submitted (1:55, 10 s)

**Say to the agent:** "Approved."

**The agent should:** immediately repeat the identical write, read the record, and
report `submitted`, with the receipt equal to the digest. It must not say counted.

**Voice-over:** "Signed and submitted. Snapshot's receipt is that same digest."

### 7. Counted, visible on Snapshot (2:05, 20 s)

**Say to the agent:** "Is it counted?"

**The agent should:** check `live.counted`, repeat the identical write once it is
true, read the record, and report `counted` only if the record says so.

**Presenter:** reload the Snapshot tab and show the new vote from `0x0c05…2751`.

**Voice-over:** "It only says counted when the hub shows a vote with that id. And
here it is on Snapshot."

### 8. Mandate refusal (2:25, 20 s)

**Say to the agent:** "Also vote For on the treasury transfer."

**The agent should:** write the vote, read the record, and report
`mandate_blocked_keyword`: nothing bound, no ceremony. It must not edit the
mandate, and it should offer to leave that decision to you.

**Voice-over:** "The mandate filters bad drafts before I'm ever asked. It's
advisory, so the real boundary is still my approval."

### 9. Closing (2:45, 15 s)

**Screen:** the README's digest table, or the route tree.

**Voice-over:** "One protocol signature, turned into a named, reviewable
operation. The agent does the reading. The owner keeps the authority. And no funds
moved."

## If a take fails

- **The agent paraphrases, or misreads a status:** re-record that shot's answer. The vote's state is in the record, so nothing is lost.
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
bloom vfs cat /petals/ballot/status.json | jq -c '{petal, version}'
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
BODY='{"vote_id":"demo-treasury","space":"'$SPACE'","proposal":"'$TREASURY'","choice":1,"reason":"Should be refused by the mandate"}'
W
REC
echo "ceremonies before=$before after=$(CER)"
```
