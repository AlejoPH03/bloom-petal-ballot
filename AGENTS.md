# ballot operating contract

`ballot` will read Snapshot governance proposals and cast owner-approved,
gasless EIP-712 votes through Bloom signing (`bloom:sign`, intent
`snapshot.vote`). **Work in progress:** proposal reads query the Snapshot hub
(read-only, both networks); nothing is submitted to a sequencer yet. The only
signing route is the temporary Step 1 `hello-sign.json`.

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
| `/petals/ballot/<network>/accounts/<wallet>/<n>/` | dir | `wallet`, `index` | lists `hello-sign.json` | — | none |
| `/petals/ballot/<network>/accounts/<wallet>/<n>/hello-sign.json` | writable, **TEMPORARY (Step 1)** | `network`, `wallet`, `index` | last attempt: status, attempt, full `typed_data`, `preimage`, `claimed_hash`, pending ids and where the owner finds them, `signature_recovers_to_account`, `signature_sha256`, timestamps, and `verify` commands | body empty or `{"attempt": n}`: requests one exact `snapshot.vote` signature over a fixed test vote; nothing is submitted | `bloom:sign`, `bloom:store`, `bloom:vfs.read` |

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
`https://hub.snapshot.org/graphql`, the only `[[net.allow]]` entries);
route values travel only as GraphQL variables.

### Temporary signing route

`hello-sign.json` is a pipeline proof (docs/plan-hello-signature.md). Use
`bloom vfs write <path> --data '{"attempt":0}'` and retry with the **same**
body after each owner approval. A pending write fails with
`approval pending (<id>)`; read the file to see where the owner finds the
ceremony. Before the owner approves, run the read's `verify` commands: they
recompute the digest with Bloom's own `/tools/eip712/hash`, and the result
must equal the exact hash on the ceremony page. Signature bytes are never
stored or returned; only their sha256 and whether they recover to the
account's `address.evm`. See docs/known-limitations.md.

## Development skill

Petals are authored against the `bloom-petal-development` skill at
`.agents/skills/bloom-petal-development/SKILL.md` (written by `petal new`).
Load it before extending this Petal. It is part of the Petal package, so
editing it changes the package hash; Bloom does not serve it under
`/petals/ballot/`.

Route files own their parameter parsing and behaviour. `route/src` holds typed
helpers only (`Network`, input validators, the hub GraphQL transport in
`hub.rs`, proposal queries and projections in `proposals.rs`, EIP-712 in
`snapshot.rs`); it never dispatches on route identity. Recorded hub responses
and the golden vote vector used by the tests are in `docs/snapshot-testnet/`. Run `bash scripts/check-route-architecture.sh` and `petal check`
before pushing.
