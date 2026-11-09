# ballot operating contract

For agents using this Petal. `ballot` reads Snapshot proposals and casts
owner-approved, gasless EIP-712 votes through Bloom signing (intent
`snapshot.vote`). Below, `A=/petals/ballot/testnet/accounts/<wallet>/<n>` and
`V=$A/votes`.

## Rules

1. Read after every write. A write is never a completed vote.
2. Never fabricate or rewrite a ceremony URL, and never put one in logs or reports.
3. Never claim a vote is counted unless the record's `status` is `counted` or `live.counted` is true.
4. Never weaken the mandate to get a vote through. A mandate refusal is final for that body.
5. Never vote on mainnet. It is disabled, and a write is refused (`mainnet_disabled`) before any external call.
6. Never write under `/wallets`, and never complete or cancel a ceremony. Only the owner approves.

## 1. Discover

| Read | Tells you |
| --- | --- |
| `/petals/ballot/status.json` | Petal, version, sign intent, Snapshot endpoints |
| `$A/inbox.json` | **Start here.** Open proposals in mandate spaces, soonest end first, with `voted` and `our_choice`. Alerts: `closing_soon` (not voted, under 24 h), `result` (closed, you voted), `delegate_voted` |
| `$A/following.json` | Open proposals a followed delegate voted on and you have not, each with `delegate_choice` and a `suggested_write` (`path` and ready `body`) |
| `$A/mandate.json` | The owner's standing rules and `last_write` |

With no mandate spaces, the inbox returns only an `explanation`. A space or
follow entry whose hub query failed shows `error`, and the others still render.
A `warning` on a follow entry means the mandate's `spaces` would refuse that
space. For anything else, name what you want, because `spaces/` and `proposals/`
list nothing: read `<network>/spaces/<space>/proposals.json`, then
`<network>/proposals/<id>.json`.

## 2. Vote

Check the proposal: `vote_type_supported` and `voting_open` must be true.
`choice` is the 1-based `number` of a `choices` entry.

```sh
bloom vfs write $V/new.json --data '{"vote_id":"<id>","space":"<space>","proposal":"0x…","choice":1,"reason":"…"}'
```

`vote_id` is `[a-z0-9-]{1,48}` and not `new`. `reason` is optional, up to 500
characters. Unknown fields are refused, and the body is at most 4 KiB. A
`suggested_write` body is a normal body: writing it only starts the owner's
approval.

## 3. Read after write

Through the mount a write runs detached and always appears to succeed. Through
`bloom vfs write` it returns its error. In both cases, read:

1. `bloom vfs cat $V/new.json | jq .last_write` shows `body_sha256` (compare it with the sha256 of your body), `outcome`, `code`, `message`, `vote_id` and `record`.
2. `bloom vfs cat $V/<vote_id>.json` shows `status`, `next`, `reason_code` and `detail`.

| Result | Meaning | Do |
| --- | --- | --- |
| `-3 refused: <reason_code>: …` | Nothing was bound or signed | Fix the request. The same `vote_id` may be reused |
| `-3 refused: mandate_*` | The mandate refused it, no ceremony | Tell the owner. See section 8 |
| `-3 approval pending (<id>)` | The owner must act | Section 4 |
| `-3 … bound to a different vote` | The `vote_id` is taken by another body | Use a new `vote_id` |
| `-4` | Transient | Retry the identical write |
| `0`, `status: submitted` | The sequencer accepted it | Identical write until `counted` |
| `0`, `status: counted` | The hub shows it | Done |

## 4. Forward approval to the human

`-3 approval pending (<id>)` is one of two ceremonies, and the route cannot tell
which. The first signing attempt by a new package hash is a policy update, then
each vote has an exact signing request. The record lists both owner locations in
`approval.pending.owner_finds_ceremony_at`:

- `/wallets/<wallet>/policy-updates/pending/<id>/approval_challenge.json`
- `/petal-signing-requests/<id>.json`

Tell the owner which path exists, the `<id>`, and what they are approving. The
owner opens `ceremony_url` from that file. If you must relay the link, copy it
unchanged to the owner only, and keep it out of logs, reports and files.

Keep the owner's window in mind: an approval lasts 5 minutes, and Signer has
refused one about 90 s before expiry. A retry before approval returns the same
pending id and opens no new ceremony.

## 5. Verify before the owner approves a signing request

The ceremony shows only a hash (`docs/known-limitations.md` §1). Before approval:

1. Show the owner `summary` (space, proposal title, choice label, reason) from `$V/<vote_id>.json`.
2. Run the two commands in `verify.commands`. Bloom's own tool recomputes the digest from the readable `typed_data`.
3. `out.hex` must equal `verify.expected_out_hex` and the ceremony's hash (`exact_ordered_hashes`, shown there without `0x`).

If they differ, tell the owner not to approve.

## 6. Identical retry

After the owner approves, repeat the **identical** write promptly. The same
space, proposal, choice and reason under the same `vote_id` resumes from the
current state, ignoring whitespace and key order. It never signs with a
different timestamp and never submits twice. The vote takes one write per stage:

```text
write -> approval pending (policy, first use of a hash only)
write -> approval pending (signing)
write -> submitted
write -> counted
```

## 7. Expiry and final states

`expired` (`proposal_closed`, `timestamp_stale`, or `approval_expired`, which is
`inferred` from a bare denial) and `rejected_by_snapshot` never change. To vote
again, write the same vote under a **new** `vote_id`. That freezes a new
timestamp and opens a new ceremony. Re-voting replaces your earlier Snapshot
vote, and needs the owner's approval like any vote.

## 8. Done and mandate

**Done.** `submitted` means the sequencer accepted the vote and `receipt.id`
equals `digest`. `counted` means the hub shows a vote with that id and choice.
`live.counted` checks the hub without writing, and one more identical write
records `counted`. Report only what the record says.

**Mandate.** A write replaces the whole mandate, and `{}` clears it. Fields:
`spaces` (empty means any), `blocked_keywords`, `require_reason`,
`max_votes_per_day` (1-100, rolling 24 h), `follow` (space to delegate address).
It is advisory: stored by this Petal and editable by you, so it is not a
security boundary. Every vote still needs the owner's approval. A vote already
bound is never re-checked.

`mandate_space_not_allowed`, `mandate_blocked_keyword`, `mandate_reason_required`
and `mandate_daily_limit` are final for that body. Do not edit the mandate to
get past them. Report the refusal and ask the owner. The one thing you may fix
yourself is `mandate_reason_required`: a body with a reason is a new body.

## 9. Route reference

| Route | Kind | Read | Write | Capabilities |
| --- | --- | --- | --- | --- |
| `status.json` | file | Petal, version, sign intent, hub and sequencer URLs | | none |
| `<network>/` | dir | `spaces/`, `proposals/`, `accounts/`. `network` is `testnet` or `mainnet` | | none |
| `<network>/spaces/`, `<network>/proposals/` | dir | Empty, reached by exact path | | none |
| `<network>/spaces/<space>/` | dir | `proposals.json`. `space` matches `[a-z0-9-]+(\.[a-z0-9-]+)+`, at most 64 bytes | | none |
| `<network>/spaces/<space>/proposals.json` | file, cached 30 s | Active proposals plus the 10 most recent, newest first: `id`, `title`, `type`, `choices`, `start`, `end`, `state`, `scores_total`, `vote_type_supported`, `voting_open` | | `bloom:http` |
| `<network>/proposals/<id>.json` | file, cached 30 s | One proposal, `body` capped at 8192 bytes with `truncated`. An unknown id is `not found` | | `bloom:http` |
| `<network>/accounts/…/<n>/` | dir | `inbox.json`, `following.json`, `mandate.json`, `votes/` | | none |
| `…/inbox.json` | file, cached 5 s | Section 1, first 10 mandate spaces | | `bloom:http`, `bloom:store`, `bloom:vfs.read` |
| `…/following.json` | file, cached 5 s | Section 1, first 10 `follow` entries | | `bloom:http`, `bloom:store`, `bloom:vfs.read` |
| `…/mandate.json` | writable, uncached | `mandate`, `notes`, `updated_ms`, `advisory`, `body_schema`, `last_write` | Mandate, at most 8 KiB | `bloom:store` |
| `…/votes/` | dir | `new.json` plus one `<vote_id>.json` per vote | | `bloom:store` |
| `…/votes/new.json` | writable, uncached | `body_schema`, `rules`, `last_write` | Section 2 | `bloom:http`, `bloom:sign`, `bloom:store`, `bloom:vfs.read` |
| `…/votes/<vote_id>.json` | file, cached 5 s | `status`, `next`, `reason_code`, `detail`, `inferred`, `summary`, `typed_data`, `preimage`, `digest`, `verify`, `approval`, `signature_sha256`, `receipt`, `counted`, `history`, `live` | | `bloom:http`, `bloom:store` |

Hub failures are `backend` errors with a short, sanitized message. `voting_open`
can be 30 s stale and does not check voting power. Signature bytes are never
readable: reads show at most `signature_sha256`.

## Developing this Petal

Load `.agents/skills/bloom-petal-development/SKILL.md` first. Route files own
their parsing and behaviour. `route/src` holds typed helpers and never
dispatches on route identity. In `route/src/votes/`, `flow.rs` is the pure state
machine over the `VoteHost` trait and `host.rs` is the only file that calls the
Petal SDK. Design: `docs/plan-votes.md`, `docs/plan-mandate-inbox.md`. Run
`bash scripts/check-route-architecture.sh` and `petal check` before pushing.
