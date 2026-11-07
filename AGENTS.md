# ballot operating contract

`ballot` reads Snapshot governance proposals and casts owner-approved,
gasless EIP-712 votes through Bloom signing (`bloom:sign`, intent
`snapshot.vote`). Proposal reads work on both networks. Votes are cast on
**testnet only**: a mainnet vote is refused (`mainnet_disabled`) before any
external call. Design: `docs/plan-votes.md`.

## Routes

| Route | Kind | Parameters | Read | Write | Capabilities |
| --- | --- | --- | --- | --- | --- |
| `/petals/ballot/` | dir | — | lists `status.json`, `testnet/`, `mainnet/` | — | none |
| `/petals/ballot/status.json` | file | — | static JSON: petal, version, sign intent, hub and sequencer URLs per network | — | none |
| `/petals/ballot/<network>/` | dir | `network` = `testnet` \| `mainnet` | lists `spaces/`, `proposals/`, `accounts/`; any other name is `not found` | — | none |
| `/petals/ballot/<network>/spaces/` | dir | `network` | empty: spaces are reached by exact path (below) | — | none |
| `/petals/ballot/<network>/spaces/<space>/` | dir | `space` = Snapshot space id, `[a-z0-9-]+(\.[a-z0-9-]+)+`, ≤ 64 bytes | lists `proposals.json`; an invalid id is `invalid` | — | none |
| `/petals/ballot/<network>/spaces/<space>/proposals.json` | file, cached 30 s | `network`, `space` | `{network, space, fetched_at, proposals: [...]}`: the space's active proposals plus its 10 most recent, each once, newest first. Each has `id`, `title`, `type`, `choices` (`[{number, label}]`, 1-based), `start`, `end`, `state`, `scores_total`, `vote_type_supported`, `voting_open`. An unknown space gives an empty list | — | `bloom:http` |
| `/petals/ballot/<network>/proposals/` | dir | `network` | empty: proposals are reached by exact path (below) | — | none |
| `/petals/ballot/<network>/proposals/<id>.json` | file, cached 30 s | `proposal` = `0x` + 64 lowercase hex | `network`, `fetched_at`, `id`, `space`, `author`, `title`, `body` (first 8192 bytes, cut on a UTF-8 boundary), `truncated`, `choices`, `type`, `start`, `end`, `snapshot`, `state`, `scores`, `scores_total`, `votes`, `vote_type_supported`, `voting_open`. Unknown id is `not found` | — | `bloom:http` |
| `/petals/ballot/<network>/accounts/` | dir | `network` | wallets (listed by Bloom) | — | none |
| `/petals/ballot/<network>/accounts/<wallet>/` | dir | `wallet` (Bloom wallet id) | numbered accounts (listed by Bloom) | — | none |
| `/petals/ballot/<network>/accounts/<wallet>/<n>/` | dir | `wallet`, `index` | lists `votes/` | — | none |
| `…/<n>/votes/` | dir | `network`, `wallet`, `index` | `new.json` plus one `<vote_id>.json` per vote of this account | — | `bloom:store` |
| `…/<n>/votes/new.json` | writable, uncached | `network`, `wallet`, `index` | `body_schema`, `rules`, `last_write` (marker of the latest write: `at_ms`, `body_sha256`, `canonical_body_sha256`, `vote_id`, `outcome`, `code`, `message`, `record`) | `{vote_id, space, proposal, choice, reason?}` ≤ 4 KiB: checks, freezes, signs (owner approval), submits to the sequencer | `bloom:http`, `bloom:sign`, `bloom:store`, `bloom:vfs.read` |
| `…/<n>/votes/<vote_id>.json` | file, cached 5 s | `vote_id` = `[a-z0-9-]{1,48}`, not `new` | `status`, `next`, `reason_code`, `detail`, `inferred`, `summary` (space, proposal and title, choice and label, reason, from, times), `typed_data`, `preimage`, `digest`, `verify`, `approval`, `signature_sha256`, `receipt`, `counted`, `history`, `live` (hub check, no store write). Unknown id is `not found` | — | `bloom:http`, `bloom:store` |

### Finding proposals

The hub has thousands of spaces, so `spaces/` and `proposals/` do not list
anything: name what you want. Pick a space id (the ENS-style name in the
Snapshot URL, e.g. `aavedao.eth`, or our testnet space
`bloom-ballot-182c.eth`), read `<network>/spaces/<space>/proposals.json`, then
read `<network>/proposals/<id>.json` with an `id` from that list:

```sh
bloom vfs cat /petals/ballot/testnet/spaces/bloom-ballot-182c.eth/proposals.json
bloom vfs cat /petals/ballot/testnet/proposals/0x5fa4f371c47a9691128eaa0bc079abed859f73b9f2e0afb453ff5a7d5b752607.json
```

`vote_type_supported` is true only for `single-choice` and `basic` proposals,
the types whose vote is one numbered `choice`. `voting_open` is true when the
hub says `active` and the read time (`fetched_at`) is before `end`; it can be
up to 30 s stale (the read cache) and does not check voting power. Times (`start`, `end`) are always the hub's own
values. Hub failures (GraphQL errors, bad status, a response over 512 KiB for
a listing or 256 KiB for a proposal) are `backend` errors with a short,
sanitized message. Both reads POST only to the network's fixed hub endpoint
(`https://testnet.hub.snapshot.org/graphql` or
`https://hub.snapshot.org/graphql`); route values travel only as GraphQL
variables. The only other `[[net.allow]]` entry is the testnet sequencer
(`POST https://testnet.seq.snapshot.org/`), used by `votes/new.json`.

### Casting a vote (agents)

`V=/petals/ballot/testnet/accounts/<wallet>/<n>/votes`

1. **Choose.** Read the proposal (`<network>/proposals/<id>.json`): it must
   have `vote_type_supported` and `voting_open` true. `choice` is the 1-based
   `number` of a `choices` entry.
2. **Write** `bloom vfs write $V/new.json --data '{"vote_id":"<id>","space":"<space>","proposal":"0x…","choice":N,"reason":"…"}'`.
   Pick a fresh `vote_id` (`[a-z0-9-]{1,48}`, not `new`). `reason` is optional
   (missing = `""`, at most 500 characters); unknown fields are refused.
3. **Read after every write.** Through the mount a write runs detached and
   always "succeeds"; through `bloom vfs write` it returns its error. Either
   way, `bloom vfs cat $V/new.json` shows `last_write` (compare its
   `body_sha256` with the sha256 of your body), and `$V/<id>.json` shows the
   vote's `status` and `next`.
4. **Refused** (`-3 refused: <reason_code>: …`): nothing was bound or signed.
   Fix the request; you may reuse the same `vote_id`.
5. **Approval pending** (`-3 approval pending (<id>)`): the first time a new
   build signs, this is Machine's policy-update ceremony for the package hash;
   then the exact signing ceremony. The owner opens it from
   `/wallets/<wallet>/policy-updates/pending/<id>/approval_challenge.json` or
   `/petal-signing-requests/<id>.json`. **Before approving the signing
   ceremony**, run the two `verify` commands from `$V/<id>.json`: Bloom's own
   `/tools/eip712/hash` recomputes the digest from the readable `typed_data`,
   and `out.hex` must equal `verify.expected_out_hex` and the ceremony's exact
   hash (`exact_ordered_hashes`, shown without `0x`). The ceremony page shows
   only that hash (docs/known-limitations.md §1). After approval, **retry the
   identical write promptly** (5-minute window; Signer may refuse ~90 s early).
   A retry before approval returns the same pending id.
6. **Identical body = resume.** The same vote (space, proposal, choice,
   reason; whitespace and key order ignored) under the same `vote_id` resumes
   from the current state: it never signs twice with a different timestamp
   and never submits twice. A different body under a bound `vote_id` is
   refused (`-3 … bound to a different vote`).
7. **Done means the record says so.** Success (exit 0) means the handler
   finished: `submitted` = the Snapshot sequencer accepted the vote
   (`receipt.id` equals `digest`, which is Snapshot's vote id); `counted` = the
   hub shows a vote with that id and choice. `live.counted` on the read checks
   the hub without writing; one more identical write records `counted`.
8. **Transient** (`-4`): retry the identical write. A vote that was posted
   is checked on the hub before any re-post.
9. **Final**: `expired` (`proposal_closed`, `timestamp_stale` — the frozen
   timestamp must reach the sequencer within 6 days — or `approval_expired`,
   which is `inferred` from a bare denial) and `rejected_by_snapshot` never
   change. To vote again, write the same vote under a **new** `vote_id`: that
   freezes a new timestamp and opens a new ceremony. Re-voting replaces your
   earlier Snapshot vote.

Signature bytes are never readable: the signature lives only in the store's
secrets namespace (`creds/votes/…`) between signing and submission, then is
deleted; reads show at most `signature_sha256`.

## Development skill

Petals are authored against the `bloom-petal-development` skill at
`.agents/skills/bloom-petal-development/SKILL.md` (written by `petal new`).
Load it before extending this Petal. It is part of the Petal package, so
editing it changes the package hash; Bloom does not serve it under
`/petals/ballot/`.

Route files own their parameter parsing and behaviour. `route/src` holds typed
helpers only (`Network`, input validators, the hub GraphQL transport in
`hub.rs`, proposal queries and projections in `proposals.rs`, EIP-712 in
`snapshot.rs`, the vote flow in `votes/`: `flow.rs` is the pure state machine
over the `VoteHost` trait, `host.rs` the only file that calls the Petal SDK);
it never dispatches on route identity. Recorded hub responses
and the golden vote vector used by the tests are in `docs/snapshot-testnet/`. Run `bash scripts/check-route-architecture.sh` and `petal check`
before pushing.
