# Plan: the owner's mandate (`mandate.json`) and the agent's inbox (`inbox.json`)

**Status: PLAN (2026-11-08), implemented the same day.** Revisions as in
`docs/plan-votes.md` (Petal SDK `ba76944`, Bloom `433a0daf`), plus the vote
flow at `db8c93e`. Hub queries are recorded under
`docs/snapshot-testnet/responses/` (FINDINGS.md §11).

## Honest scope

The mandate is **advisory**. It lives in the Petal's store and the same agent
it constrains can rewrite it through `mandate.json`. It filters bad drafts
*before* the owner is asked; it does not replace the owner: every vote still
needs the owner's passkey approval (docs/plan-votes.md §3). An owner-signed,
binding mandate (one the agent cannot edit) is future work. `follow` is
stored and validated only; `following.json` is not in scope.

## Routes

`[network]/accounts/[wallet]/[index]/$index` lists `inbox.json`,
`mandate.json`, `votes/`.

| Route | Kind | Spec | Caps (= imports) |
| --- | --- | --- | --- |
| `mandate.json` | writable | `petal::write_spec().caps(&["bloom:store"])` (SDK `lib.rs:1272`; `write_async`, no cache) | `bloom:store` |
| `inbox.json` | file, 5 s cache | `petal::account_read_spec().caps(&["bloom:http","bloom:store","bloom:vfs.read"])` (SDK `lib.rs:1259-1263`) | `bloom:http`, `bloom:store`, `bloom:vfs.read` (`address_evm`, research-signing.md:362-379) |
| `votes/new.json` | unchanged | unchanged | unchanged: the mandate is read with `bloom:store`, already imported |

No manifest change: the hub `[[net.allow]]` entries and `[store]
namespaces = ["state"]` already cover both routes.

## Mandate

**Body** (≤ 8192 bytes, a JSON object, unknown fields refused; every field
optional; the stored mandate is replaced as a whole):

| Field | Rule | Default |
| --- | --- | --- |
| `spaces` | ≤ 50, each `validate_space` (`route/src/lib.rs:118`), duplicates dropped | `[]` = no space restriction |
| `follow` | object ≤ 50 entries: key `validate_space`, value `0x` + 40 hex, stored EIP-55 checksummed (`snapshot::checksum_address`) | `{}` |
| `blocked_keywords` | ≤ 50, each 1..=64 chars after trimming, matched case-insensitively | `[]` |
| `require_reason` | bool | `false` |
| `max_votes_per_day` | integer 1..=100 | absent = no limit |

`{}` (only whitespace and braces) **clears** the mandate (`store_del`, SDK
`lib.rs:602`; Bloom's delete succeeds on a missing key,
`private_store.rs:127-135`).

**Store** (state namespace, account-scoped like the votes, plan-votes §1):
`mandate/<network>/<wallet>/<index>.json` =
`{schema:"ballot.mandate.v1", spaces, follow, blocked_keywords, require_reason, max_votes_per_day, updated_ms}`;
marker `markers/<network>/<wallet>/<index>/mandate.json` =
`{at_ms, body_sha256, outcome: saved|cleared|invalid_body|transient_error, code, message}`
(the votes marker shape, `route/src/votes/record.rs:241`). The write returns
`-3` for an invalid body, `-4` for a store failure.

**Read**: `{schema, network, wallet, index, mandate|null, notes[], advisory,
body_schema, last_write}`. `notes` explain the effect: empty `spaces` (no
restriction, and the inbox needs spaces), more than 10 spaces (the inbox
reads the first 10), no `max_votes_per_day`, `follow` stored only.

**Decision (flag):** an empty or absent `spaces` means "no space
restriction", not "nothing allowed"; otherwise a mandate with only
`blocked_keywords` would refuse every vote.

## Enforcement in `votes/new.json`

Only in `draft()` (`route/src/votes/flow.rs:200`), i.e. only for an unbound
vote_id; the bound path never re-reads the mandate, so an identical retry of
a bound vote is unaffected (plan-votes §3). Every refusal goes through the
existing `refuse()`: `-3 refused: <code>: <detail>`, a `refused` record,
nothing bound, no `sign` call, no ceremony.

| Order | Check | Code | Where |
| --- | --- | --- | --- |
| after the mainnet gate (`:202`), **before any hub call** | `spaces` non-empty and `body.space` not in it | `mandate_space_not_allowed` | before `hub_proposal` (`:215`) |
| after the existing hub checks (proposal, space, open, type, choice, vp; `:215-251`) | a keyword occurs in the proposal title or body (lowercased; body as the hub returns it for `PROPOSAL_QUERY`, `proposals.rs:19`) | `mandate_blocked_keyword` (detail names the keyword) | the `mandate_check` hook (`:98`, `:261`) |
| ″ | `require_reason` and `reason == ""` | `mandate_reason_required` | ″ |
| ″ | `max_votes_per_day` set and the account's **bound** votes with `drafted_ms > now − 24 h` ≥ it | `mandate_daily_limit` | ″; bindings listed with `store_list` (`flow.rs:870`) and read one by one |

The daily limit is a **rolling 24 h** window over `binding.drafted_ms`
(`record.rs:95`), per network and account; refused votes never count. Two
concurrent drafts under different vote_ids hold different leases, so they
can exceed the limit by one (documented, not fixed). A store failure while
reading the mandate or counting is `-4`, nothing bound.

## Inbox

`inbox.json` read, no store writes:

1. Account address (`address_evm`) and the mandate. No mandate or empty
   `spaces` → `{…, spaces: [], explanation: "set mandate.json spaces …"}`.
2. For each of the first **10** mandate spaces, at most **2** hub calls
   (bound: 3):
   - `ballot_inbox.graphql`: `open` = `proposals(first: 20, where:{space,
     state:"active"}, orderBy:"end", asc)` and `recent` =
     `proposals(first: 10, where:{space}, orderBy:"created", desc)`, each with
     `id title type choices start end state scores scores_total`;
   - `ballot_voted.graphql`: `votes(first: 50, where:{voter,
     proposal_in:[ids of open ∪ recent closed]}) { id choice created proposal { id } }`
     — skipped when there are no ids. `proposal_in: [String]` and a
     case-insensitive `voter` are confirmed on the testnet hub (FINDINGS.md §10-11).
3. Per space: `open` = the `open` proposals with `voting_open`
   (`proposals.rs:50`): `id, title, type, vote_type_supported, closes_in_s,
   voted, our_choice`. Interpretation: "≤ 20 proposals per space" bounds the
   open list; the 10 most recent are queried only for results.
4. `alerts`: `closing_soon` for an open proposal with `closes_in_s < 86400`
   that this account has not voted on; `result` for each `closed` proposal in
   `recent` this account voted on: `winning_choice` = the label with the
   highest score (`null` without scores; `tie: true` when several share it),
   `our_choice`, `our_choice_won`.
5. A hub failure for one space puts `error` on that space only.

Document: `{schema:"ballot.inbox.v1", network, account:{wallet, index,
address}, fetched_at, spaces:[{space, open:[…], error?}], alerts:[…],
notes, explanation?}`.

## Code layout

- `route/src/mandate.rs`: body parsing/validation, the stored mandate, notes,
  the pure checks (space, keyword, reason, daily count) and the read/write
  documents, generic over `KvStore` (`votes/store.rs:129`); `KvStore` gains
  `store_del`.
- `route/src/inbox.rs`: queries, projections, alerts, the document; host
  calls through a small `InboxHost` trait (`hub_inbox`, `hub_voted`), so the
  logic is tested against the recorded responses.
- `flow.rs`: `draft()` loads the mandate once; the space check before the
  hub; `mandate_check` gets the mandate, the proposal and the daily count.
- Route files `mandate.json.rs`, `inbox.json.rs`; `[index]/$index.rs` lists
  the three children.

## Tests

Mandate body (limits, unknown fields, `{}` clears, checksum, dedupe, 8 KiB);
each enforcement code with "no hub call" for the space check and "no sign
call" for all; a bound vote's retry ignores a new mandate; the 24 h boundary;
inbox projection from `ballot_inbox_testnet.json` + `ballot_voted_testnet.json`
(test proposal `voted: true`); synthetic results (win, loss, tie, no scores);
`closing_soon`; one space failing while another renders; no mandate →
explanation. Then Verify from CLAUDE.md with reads only.
