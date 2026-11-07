# Plan: the vote flow (`votes/new.json`, `votes/[vote_id].json`)

**Status: PLAN (2026-11-07), implementation 2026-11-08.** Nothing here is
implemented. Every claim cites a file:line at these revisions:

| Source | Revision | Path |
| --- | --- | --- |
| Petal SDK (our pin) | `ba76944` | `~/.cargo/git/checkouts/petal-c092b35600511bdf/ba76944/crates/petal-sdk/src` (`SDK lib.rs`, `SDK claim.rs`) |
| Bloom Machine | `433a0daf` | `~/work/bloom` |
| snapshot-sequencer | `f577889` (deployed `14783ac`, FINDINGS.md:9-11) | `~/work/step0/vendor/snapshot-sequencer` |
| snapshot.js | `6ebeb61` | `~/work/step0/vendor/snapshot.js` |
| Polymarket (reference) | `1141037` | `~/work/bloom-petal-polymarket` |
| This repo | `bff96b6` | — |

## 0. Headline findings

1. **Snapshot's vote id is our EIP-712 digest.** The sequencer's id is
   `getHash(body.data, body.address)` (`snapshot-sequencer/src/ingestor.ts:119`),
   which for EVM is `_TypedDataEncoder.hash(domain, types, message)`
   (`snapshot.js/src/verify/evm.ts:17-20`), i.e. the digest Bloom signs.
   Proven on the Step 0 live vote: `docs/snapshot-testnet/golden/digest_manual.sh`
   over its message gives `0xd74075ee…a657`, the vote id the sequencer returned
   and the hub shows (FINDINGS.md:283, `responses/final_hub_check.json`).
   Consequences: (a) the digest the owner checks on the ceremony page is the
   id Snapshot records; (b) reconciliation and `counted` are an exact id match,
   not a guess; (c) re-POSTing the same signed vote can never create a second
   vote (§4).
2. **Literal `new.json` beats `[vote_id].json`.** Bloom picks the matching
   route with the most static segments (`bloom-petals/src/package.rs:1569-1585`,
   `specificity` `:4329-4339`), so a vote id `new` could never be read. Reserve
   it (`vote_id != "new"`).
3. **The record outlives a crash only if the timestamp is frozen before the
   first signature.** The binding (frozen typed data) is written with
   `store_put_new` before `sign_payload`; every retry re-signs the same bytes.
4. **Exact signing survives a lost signature, probably.** Machine replays the
   same signing operation ("Once prepared, replay the same identity so Broker
   can recover its result", `bloom-vfs/src/exact_signing.rs:478-481`; Machine
   calls `sign_exact_payload_recovering`, `:530`,
   `bloom-machine-client/src/lib.rs:886-891`). Unverified live; the design does
   not depend on it except for a crash between "signature returned" and
   "secret written".

## 1. Store design

**Scope.** Bloom gives every account its own private store
(`bloom-petals/src/private_store.rs:36`, `account_digest` `:192-199`;
SKILL.md:107-108). Keys still carry `<network>/<wallet>/<index>` as in
`hello::record_key` (`route/src/hello.rs:211`): defence in depth, and the
network is not part of Bloom's scope.

**Namespaces.** The SDK routes a key to `secrets` when `secret == true` or the
key is `creds` / starts with `creds/`, else to `state`
(`SDK lib.rs:402-403`, `namespace_for_key` `:677-683`). `store_get`,
`store_del` and `store_list` always pass `secret = false` (`:580`, `:603`,
`:613`), so **a secret is readable/deletable only under a `creds/` key**.
Bloom checks the namespace is declared and that the `secret` flag matches its
declaration (`bloom-petals/src/policy.rs:216-240`, called from
`vm.rs:2449-2464`).

Let `A = <network>/<wallet>/<index>` (wallet from `petal::wallet_account`,
already a validated single segment; `vote_id` validated by
`validate_vote_id`, `route/src/lib.rs:148-156`, plus `!= "new"`).

| Key | Namespace | Written with | Mutability | Content |
| --- | --- | --- | --- | --- |
| `votes/A/<vote_id>/binding.json` | state | `store_put_new(.., false)` (`SDK lib.rs:597`) | **immutable**, never `put`, never deleted | `{schema:"ballot.vote-binding.v1", vote_id, network, wallet, index, from, space, proposal, choice, reason, canonical_body_sha256, vote:{…8 fields incl. timestamp…}, typed_data, digest, preimage, proposal_facts:{title, type, choices, choice_label, start, end, snapshot}, drafted_ms}` |
| `votes/A/<vote_id>/record.json` | state | `store_put(.., false)` (`:592`) | mutable state | `{schema:"ballot.vote-record.v1", status, reason_code?, detail?, pending?, receipt?, submit_attempts, last_error?, history:[{status, at_ms}] (≤ 32), updated_ms}`; for `refused`, also the refused body fields |
| `votes/A/<vote_id>/lease` | state | `store_put_new` / `store_del_if_value` (`:597`, `:607`) | transient | `{token (16 random bytes hex, SDK lib.rs:654), acquired_ms}` |
| `markers/A/new.json` | state | `store_put` | overwritten by every write | last-write marker (§9) |
| `creds/votes/A/<vote_id>/signature` | **secrets** | `store_put(.., true)` | deleted once `submitted` or terminal | 65 raw signature bytes, nothing else |

**Atomic binding.** `store_put_new` is create-if-absent: Bloom's
`PrivateStore::put_new` uses `create_new_file` and maps `AlreadyExists` to
`Denied("store key already exists")` (`private_store.rs:84-104`, `:357`),
which the SDK surfaces as `SdkError::Host(HostStatus::Denied)` (SKILL.md:582-585).
The **first successful `put_new` of `binding.json` binds the vote_id**. A
`Denied` from it means "already bound": reload and take the resume path.

**Refused stays unbound.** A refusal writes only `record.json`
(`status:"refused"`) and never `binding.json`. "Bound" ≡ `binding.json`
exists. A later write with a corrected body finds no binding, re-runs the
checks and either overwrites the refused record or binds.

**Serialisation per vote_id (lease).** All of a write's store work runs under
`votes/A/<vote_id>/lease`, acquired with `store_put_new`; a stale lease
(> 120 s) is taken over with `store_del_if_value` on its exact bytes; release
with `store_del_if_value` on drop. This is Polymarket's lock
(`route/src/infra_parts/store.rs:14-60`, `:83-100`). Busy → `-3` "another
write for this vote_id is in progress; retry the same write". It prevents a
refusal racing a draft and two concurrent signs/POSTs; correctness of
submission does not rely on it (§4).

**Size bounds.** `binding.json`/`record.json` ≤ 64 KiB on read (as
`hello.rs` `MAX_RECORD_BYTES`); `store_list` bounded to 64 KiB of key names.

## 2. Clock

`petal::sdk::try_now_ms()` (`SDK lib.rs:650-652`) → `bloom:env/runtime`
`now-ms` → Bloom `component_env_now_ms` (`bloom-petals/src/vm.rs:2345-2358`):
`SystemTime::now()` in ms. **No capability:** the function has no `caps`
check (contrast `component_store_get`, `vm.rs:1046`), and SKILL.md:297 maps
`bloom:env/runtime` to no capability. It returns `0` only when
`deterministic_env` is set, which Bloom sets for metadata extraction
(`package.rs:901`), not for handlers. Already used:
`hub::now_secs` (`route/src/hub.rs:206-208`), `hello::now_ms` (`hello.rs:306`).
Rule: a missing clock or `0` is `-4 clock unavailable`; never freeze
timestamp 0. Vote timestamp = `now_ms / 1000` (seconds, like snapshot.js
`sign()`, FINDINGS.md:38).

## 3. State machine

### States

`refused` (unbound) · `drafted` · `awaiting_approval` · `signed` (normally
transient; durable so a crash between sign and submit is recoverable) ·
`submitted` (sequencer accepted, receipt id = digest) · `counted` (hub shows
vote id == digest with this choice) · `rejected_by_snapshot` · `expired` ·
`superseded` (hook only, Monday).

### Every write, in order

1. **Body** (pure, no store): ≤ 4096 bytes; a JSON object; fields
   `vote_id, space, proposal, choice, reason?`, `deny_unknown_fields`;
   `vote_id` grammar and `!= "new"`; `validate_space` / `validate_proposal_id`
   (`route/src/lib.rs:107-145`); `choice` u32 ≥ 1; `reason` ≤ 500 chars
   (Unicode scalar values), absent ≡ `""` (snapshot.js default,
   FINDINGS.md:35). Failure → `-3`, marker only (no record: the id may be
   unusable).
2. **Network gate:** `mainnet` → `refused` `mainnet_disabled`, `-3`, no
   external call (CLAUDE.md rule 7; see Conflicts).
3. **Lease.**
4. **Load** binding + record; identity = equality of the normalised tuple
   `(space, proposal, choice, reason)` (network/wallet/index are the key).
5. Table below. 6. **Marker** written on every exit (§9). 7. Release lease.

### Transition table

| Current state | Body | Handler does (external calls in **bold**) | Stores | Returns |
| --- | --- | --- | --- | --- |
| none / `refused` | any valid | **hub proposal** (`PROPOSAL_QUERY`, `proposals.rs:19`) → checks; **hub vp** → check; mandate hook (no-op today) | fail: `record`=refused{reason_code} | `-3 refused: <code>: <detail>` |
| ″ | ″ | all pass → freeze `Vote{from, space, timestamp=now, proposal, choice, reason, app:"bloom-ballot", metadata:"{}"}` → `put_new binding` → `record`=drafted → continue as `drafted` in the same call | binding, record | (from the `drafted` row) |
| `drafted`, `awaiting_approval` | identical | expiry check (§5) → **`sign_payload`** (exact, derived nonce, `action` = typed-data bytes, no hint) | see right | — |
| ″ → Pending | | | record=awaiting_approval{action_id, expires_ms, owner paths} | `-3 approval pending (<id>); owner opens …; retry the exact same write` |
| ″ → Signature | | recover == `from` (`snapshot::recover_signer`) → `put creds/…/signature` (secret) → record=signed → continue as `signed` | secret, record | (from `signed`) |
| ″ → Signature not recovering | | drop bytes | record.last_error=`signature_mismatch` (state unchanged) | `-4` |
| ″ → `Denied` | | if awaiting and `now ≥ pending.expires_ms − 120 s` → expired(`approval_expired`); else stay | record.last_error | `-3` (text names provenance/policy/not-yet-approved/`CEREMONY_RATE_LIMITED`) |
| ″ → other `SdkError` | | stay | record.last_error | `-4` |
| `signed` | identical | expiry check → **hub votes(proposal, voter)**: id == digest → submitted (reconciled). Else load secret (missing → back to `drafted` row, Machine recovers/re-signs the identical payload) → record.submit_attempts+1, last_attempt_ms (**durable before POST**) → **POST sequencer** → classify (§4) | record; delete secret on submitted/terminal | submitted: `Write`; transient: `-4 submission not confirmed; retry the same write`; permanent: `-3 rejected by Snapshot: <code>` |
| `submitted` | identical | **hub votes** → id == digest & choice match → counted; delete leftover secret | record | `Write` |
| `counted` | identical | nothing | marker only | `Write` |
| `rejected_by_snapshot`, `expired`, `superseded` | identical | nothing | marker only | `-3 <status> (<reason_code>): this vote_id is final; use a new vote_id` |
| any bound state | different | nothing; no external call | marker only | `-3 vote_id <id> is bound to a different vote (space/proposal/choice/reason differ)` |

Pre-sign refusal codes: `proposal_not_found`, `space_mismatch`
(`proposal.space.id != space`), `proposal_not_active` (`state != "active"`),
`voting_closed` (`now ≥ end`), `voting_not_started` (`now < start`, hub's
start, FINDINGS.md:313), `unsupported_vote_type` (not single-choice/basic,
FINDINGS.md:35-37), `choice_out_of_range` (`choice > choices.len()`),
`no_voting_power` (`vp == 0`), `mainnet_disabled`; Monday:
`mandate_*`. A hub failure during checks is **not** a refusal: `-4`, record
unchanged, nothing bound.

### Order of durable writes vs external calls

| Step | Durable first | Then | Crash right after → next identical write |
| --- | --- | --- | --- |
| Draft | `put_new binding` (frozen timestamp) | `record=drafted` | binding without record ⇒ treat as `drafted` |
| Sign | binding exists | `sign_payload` | re-signs the **same** payload → same request id (`SDK claim.rs:435-446,543`; `bloom-daemon/src/lib.rs:1043-1076`) |
| Signature returned | — | `put secret` → `record=signed` | lost before `put secret`: identical retry gets it back by recovery (§0.4) or a new identical-bytes signature (RFC 6979) |
| Submit | `record.submit_attempts+1` | POST | hub reconcile first; then a re-POST of the **same** bytes is harmless (§4) |
| Accepted | `record=submitted{receipt}` | `del secret` | secret leftover is deleted on the next write |

A write is never reported as a completed vote: `Write` (exit 0) means the
handler finished and the record is `submitted` or `counted`
(SKILL.md:340-346, 357-361); the record states which.

### Host trait (for testing)

`route/src/votes/flow.rs` holds the pure machine
`fn run_write(input: &VoteBody, ctx: &AccountCtx, host: &mut impl VoteHost) -> Outcome`.
`trait VoteHost { now_ms; hub_proposal; hub_vp; hub_votes; sign(&Vote) -> SignResult; post_envelope(&[u8]) -> PostResult; store_get/put/put_new/del/del_if_value/list; random16 }`.
`route/src/votes/host.rs` is the only file calling `petal::sdk::*` and
`petal::petal_use_claim`; tests use a scripted `FakeHost` that logs calls.

## 4. Sequencer idempotency and error classes

- **Same signed vote twice.** The id is a pure function of the signed data
  (`ingestor.ts:119`); `doesMessageExist(id)` → `duplicate message`
  (`:122-124`, also on DB unique violation `:264-266`). Concurrent identical
  bodies → `425 request already being processed`
  (`helpers/duplicateRequestPreventor.ts:8-19`). A voter has one row per
  proposal (`writer/vote.ts:139-192`); an equal timestamp with an equal id is
  refused (`:148-151`). So a re-POST cannot double-count.
- **Lost response.** Reconcile with the hub before any re-POST:
  `votes(where:{proposal, voter: from}) { id choice created }` (verified shape,
  FINDINGS.md:119-123). `id == digest` ⇒ `submitted` (`reconciled: true`).
- **Response shapes.** Success `{id, ipfs, relayer:{address, receipt}}`
  (`ingestor.ts:284-291`); errors `{error:"client_error"|"server_error",
  error_description}` with 400 for string rejections, 500 otherwise, 503
  maintenance (`api.ts:19-36`, `helpers/utils.ts:33-39`). Store `id`, `ipfs`,
  relayer address and `sha256(receipt)` only (the receipt is a relayer
  signature, CLAUDE.md rule 6). `id != digest` on 200 ⇒ `submitted` with
  `warning: receipt_id_mismatch`.

| `error_description` (source) | Class | Record |
| --- | --- | --- |
| `duplicate message` (`ingestor.ts:123,265`) | reconcile | hub id == digest ⇒ submitted; else stay `signed`, `-4` |
| 425 `request already being processed`; 503 `update in progress, try later`; any 5xx; `pinning failed` (`:243`); `failed to check vote validation` / `failed to check voting power` (`vote.ts:74,97`); transport error, timeout, undecodable or oversized body | transient | stay `signed`, `-4`, retry same write |
| `not in voting window` (`vote.ts:30-38`) | permanent | now ≥ end ⇒ `expired(proposal_closed)`; else `rejected_by_snapshot` |
| `wrong timestamp` (`ingestor.ts:57-67`) | permanent | ts older than 6 d ⇒ `expired(timestamp_stale)`; else `rejected_by_snapshot(clock_skew)` |
| `already voted at later time`, `already voted same time with lower index` (`vote.ts:146-151`) | permanent | `rejected_by_snapshot(newer_vote_exists)` — the `superseded` hook |
| `wrong envelope format`, `too large message`, `proposal cannot contain whitespace`, `wrong domain`, `wrong types`, `Invalid space id`, `unknown space`, `signature validation failed` (`ingestor.ts:47-117`), `too large metadata` (`:173-174`), `wrong vote format`, `unknown proposal`, `invalid choice`, `failed vote validation`, `no voting power` (`vote.ts:21-89`) | permanent | `rejected_by_snapshot(<code>)` |
| anything else | transient | stay `signed`, `-4` |

Upstream text is sanitised (`hub::sanitize`, `hub.rs:119`) and any run of
≥ 130 hex digits is redacted before it reaches a record, marker or error.

## 5. Expiry

| Limit | Source | Rule |
| --- | --- | --- |
| Proposal end | hub `end`; sequencer checks message ts and server time (`vote.ts:30-38`) | not yet `submitted` and `now ≥ end` ⇒ `expired(proposal_closed)` (checked before signing and before POST; a `signed` record reconciles on the hub first) |
| Frozen timestamp | sequencer accepts `ts ∈ [now − 6 d, now + 300 s]` (`ingestor.ts:57-67`, FINDINGS.md:62-63) | `now − ts > 6 d − 10 min` ⇒ `expired(timestamp_stale)` |
| Exact approval window | TTL 5 min from the first exact attempt (`exact_signing.rs:27`, state created `:440-460`) | before any approval id: an identical retry after 5 min resets locally and opens a new ceremony for the same request id (`:478-490`); stays `awaiting_approval` with the new pending id |
| Approval expired after an approval id was stored | Machine replays the stored id (`:478-490`, `:519-528`); suspected wedge (known-limitations.md §2) | `Denied` with `now ≥ expires_ms − 120 s` ⇒ `expired(approval_expired)` |
| Signer's early `APPROVAL_EXPIRED` (~90 s before `expiry_ms`) | known-limitations.md §4 | same rule (120 s margin) |
| `CEREMONY_RATE_LIMITED` | one live ceremony per wallet (SKILL.md:548-559) | not expiry: stay, `-3` "finish or wait out the other ceremony, then retry the same write" |
| Early retry (owner not done) | Machine returns the same pending while Broker says Prepared/AwaitingCeremony (`exact_signing.rs:356-399`, `:519-528`) | stays `awaiting_approval`, same `action_id`, no new ceremony |

**After `expired`:** the agent writes a **new vote_id** with the same
space/proposal/choice/reason. That re-runs the checks, freezes a new
timestamp, so a new payload, a new derived nonce and request id, and a new
ceremony; this is the escape hatch hello-sign needed `{"attempt":n}` for. The
old record is final.

## 6. `counted`

**Recommendation: computed live on read, persisted only by a write.**
`[vote_id].json`'s read calls the hub and returns
`live: {checked_at, counted, hub_vote:{id, choice, created} | null, superseded_hint}`
without writing the store. The stored status moves to `counted` only when an
identical write runs on a `submitted` record.

Why: a read handler gives "safe instructions or current non-secret state"
(SKILL.md:80-82); the spec is not a side-effecting read
(`side_effecting_read: false` by default, `SDK lib.rs:1178-1186`, set only by
`chain_read_spec` `:1265-1270`); Bloom caches reads (TTL, `vm.rs:750`;
invalidated by writes, `bloom-vfs/src/router.rs:343-345`, `cache.rs:77-97`),
so a read that mutates state would mutate it at unpredictable times; and
"durable state does not overstate external completion" (SKILL.md:637) is kept
because the stored status only advances on positive hub evidence observed by
a write. A hub failure on read gives `live.checked: false` with the sanitised
error, not `-4`.

## 7. Capabilities and manifest

| Route | Kind | Spec | Caps (= imports) | Why |
| --- | --- | --- | --- | --- |
| `[network]/accounts/[wallet]/[index]/$index` | dir | `static_dir_spec()`, `ctx_list` | none | lists `votes/` |
| `…/votes/$index` | dir | `store_dir_spec().caps(&["bloom:store"])` (`SDK lib.rs:1235`) | `bloom:store` | `store_list` → `writable("new.json")` + `file("<id>.json")` per binding/record (`SDK lib.rs:1057` `ctx_list`, `:1691-1703`) |
| `…/votes/new.json` | writable | `signing_write_spec("snapshot.vote").caps(&["bloom:http","bloom:sign","bloom:store","bloom:vfs.read"])` (`:1279`) | http, sign, store, vfs.read | hub + sequencer; `sign_payload`; records/secret; `address_evm` → `vfs_read` (research-signing.md:362-379) |
| `…/votes/[vote_id].json` | file | `account_read_spec().caps(&["bloom:http","bloom:store"])` (`:1259-1263`, TTL 5 s) | http, store | record + live hub check; `from` comes from the binding, so no `vfs.read`; `wallet_account` makes no host call |

Imports map `bloom:http/fetch`→`bloom:http`, `bloom:store/kv`→`bloom:store`,
`bloom:sign/signing`→`bloom:sign`, `bloom:vfs/readwrite` (read only) →
`bloom:vfs.read`, `bloom:env/runtime`→none (SKILL.md:289-297). A route with
parameters must not declare a capability it does not import (SKILL.md:250-263),
so `store_dir_spec`'s `vfs.read` and `account_read_spec`'s `vfs.read` are
removed. Confirm each with `wasm-tools component wit` after `petal build`.

**`petal.toml` diff** (`[caps]` and `[sign]` unchanged):

```diff
-# Snapshot hub GraphQL, one table per network (route/src/lib.rs
-# Network::hub_graphql_url). Exact path, no wildcard. Key names as in
-# bloom-petal-hyperliquid petal.toml. The sequencers are not listed: no route
-# submits anything yet.
+# Snapshot hub GraphQL (reads, vp, votes) and sequencer (vote submission),
+# one table per host (route/src/lib.rs Network::hub_graphql_url,
+# Network::sequencer_submit_url). Exact paths, no wildcard.
 [[net.allow]]
 binding = "testnet"
 …
+[[net.allow]]
+binding = "testnet-sequencer"
+host = "testnet.seq.snapshot.org"
+methods = ["post"]
+paths = ["/"]
+
+[[net.allow]]
+binding = "mainnet-sequencer"
+host = "seq.snapshot.org"
+methods = ["post"]
+paths = ["/"]
+
 [sign]
 allowed_intents = ["snapshot.vote"]

-# Public attempt records only. No secret namespace: no secret is stored and
-# signature bytes are never persisted.
+# state: vote bindings, records, leases and the last-write marker.
+# secrets: a vote's signature between signing and submission, only under
+# creds/votes/… (the SDK reads/deletes a secret only via a creds/ key).
 [store]
 namespaces = ["state"]
+secret_namespaces = ["secrets"]
```

Binding names allow `-` (`bloom-petals/src/policy.rs:270-281`). A path rule
without `*` matches only itself (SKILL.md:304-308), so the request URL must be
exactly `https://<host>/`: add `Network::sequencer_submit_url()` returning the
trailing-slash URL (`sequencer_url`, `route/src/lib.rs:71-76`, has none and
stays for `status.json`). Same form as Hyperliquid/Polymarket
`secret_namespaces` (research-signing.md:248-251, 275-277). Bloom requires a
store namespace with `bloom:store` (`package.rs:4122-4130`).

## 8. Readable without trusting Ballot

`[vote_id].json` exposes, all public: the readable summary (space, proposal
id and title, choice number **and label**, reason, from, network, account),
the frozen `typed_data` (with `EIP712Domain` and `primaryType`, as
`Vote::typed_data`, `snapshot.rs:101-132`), `preimage`, `digest`, and
`verify` built like `hello::verify_section` (`hello.rs:226-236`):

```sh
bloom vfs cat /petals/ballot/<net>/accounts/<w>/<n>/votes/<id>.json | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-votes/in.json
bloom vfs cat /tools/eip712/hash/ballot-votes/out.hex
```

Bloom's own tool hashes with alloy (`bloom-vfs/src/handlers/tools.rs:17`,
`539-545`, FINDINGS.md:212-213). The owner compares `out.hex` with the
ceremony's "The exact plan that was reviewed" → `exact_ordered_hashes`
(shown without `0x`), as Step 1 proved (known-limitations.md:87-111,
`~/work/evidence/step1-hello-sign.json`). After submission the same value is
Snapshot's vote id (§0.1), so `receipt.id == digest` and the hub's vote id
close the loop. Approval pointers: `pending.owner_finds_ceremony_at` lists
both owner locations, since the route cannot tell a policy-update id from an
exact request id (`hello::Pending`, `hello.rs:117-139`). No field can hold
signature bytes; at most `signature_sha256`.

## 9. Mounted writes vs `bloom vfs write`

`bloom vfs write` runs the write synchronously and returns its error; the
mount runs it detached and the writer always sees success (SKILL.md:340-346,
586-592). So every outcome must be readable:

- `markers/A/new.json`, shown by `new.json`'s read as `last_write`:
  `{at_ms, body_sha256 (raw bytes), canonical_body_sha256, vote_id|null,
  outcome: "invalid_body"|"busy"|"refused"|"conflict"|"approval_pending"|
  "submitted"|"counted"|"rejected_by_snapshot"|"expired"|"transient_error"|…,
  code (0 or -1..-4), message (the exact text the write returned), record:
  "votes/<id>.json"|null}`. One `finish()` writes it on every exit; it is the
  only trace of an `invalid_body` or `busy` write.
- the record carries status, `reason_code`, `last_error`, `history`.

Only a store failure while writing the marker escapes both (then Machine logs
`mount.adapter.async_command_outcome_deferred`, SKILL.md:586-592).

`new.json`'s read: `{schema, body_schema, rules (limits, reserved id, states,
retry and expiry rules, the verify procedure), last_write}`.

## 10. Removing `hello-sign.json`

Delete `route/files/[network]/accounts/[wallet]/[index]/hello-sign.json.rs`
and `route/src/hello.rs`; `[index]/$index.rs` lists `dir("votes")`
instead of `writable("hello-sign.json")`. Move into shared modules:

| From `hello.rs` | To | Change |
| --- | --- | --- |
| `account_address` (`:282-286`) | `votes/host.rs` | none |
| claim + `sign_request` + `action` + `sign_payload` (`:357-377`) | `votes/host.rs::sign_exact(ctx, wallet, &Vote)` | `ClaimNonce::Derived` only; no `Attempt`; `approval_hint` stays `None` |
| `Pending` (`:117-139`) | `votes/record.rs` | unchanged |
| `verify_section` (`:226-236`) | `votes/read.rs` | session `ballot-votes`, path parameter |
| `load_record`/`save_record` (`:288-304`) | `votes/store.rs` | generic JSON get/put/put_new + lease |
| `HELLO_*`, `Attempt`, `Record`, `Status` | deleted | — |

Kept: `snapshot.rs` and its golden tests. Update `AGENTS.md`, `README.md`,
`petal.toml` comments, `scripts/dev-cycle.sh` step 3 text ("expect sign,
store, vfs and http only on votes/new.json").

## 11. Read-route tweak

Replace `ballot_can_vote` (`proposals.rs:42-44`, fields `:102`, `:210`,
uses `:108`, `:222`) with:

- `vote_type_supported: bool` = type is `single-choice` or `basic`
  (same predicate, renamed);
- `voting_open: bool` = `state == "active"` and `now < end`, with `now` the
  `fetched_at` both routes already pass (`[proposal].json.rs:77-81`,
  `proposals.json.rs:108-113`); `false` when the clock is unavailable.

One shared `proposals::voting_open(state, end, now)` is used by both reads
and by the write's checks. Tests: demo (`0x5fa4…`) open at `now <
1794594112`, closed (`0xb2a3…`, `state:"closed"`, end 1793989914) not open,
in `responses/ballot_proposals_testnet.json`. AGENTS.md:18-20, 39-41 updated.

## 12. Test plan

### Host unit tests (`cargo test`)

- **Body:** 4096/4097 bytes; non-object; unknown field; missing field;
  `choice` 0, negative, string; `reason` 500 vs 501 chars with multibyte
  text; `vote_id` `new`, uppercase, 49 chars; bad space/proposal; trailing
  newline accepted; `reason` absent ≡ `""`.
- **State machine (FakeHost, table-driven, one case per row of §3):** asserts
  the returned code/message, the stored record/binding, the marker, and the
  exact host-call log (e.g. "different body under a bound id makes zero
  external calls"; "refused never writes binding"; "signature is written
  only with secret=true under creds/ and deleted after submitted").
- **Crash injection:** the fake store fails after write k for each k in the
  draft→sign→submit sequence; the next identical write converges without a
  second POST when the hub already shows the vote, and with exactly one
  re-POST of identical bytes otherwise.
- **Binding:** `put_new` on an existing key → resume path; refused then
  corrected body → binds; lease busy → `-3`; stale lease taken over.
- **Envelope:** golden `message.json` + `signature.hex` → our envelope
  parses equal to `golden/envelope.json`; `data` has no `EIP712Domain`/
  `primaryType`; compact `types` equals the FINDINGS.md:56-57 string;
  its sha256 is `fb8fa981…` (FINDINGS.md:197).
- **Digest = Snapshot id:** new fixture
  `golden/live_vote_message.json` (the public Step 0 vote message) →
  `0xd74075ee…a657` (FINDINGS.md:283).
- **Classification:** every `error_description` in §4 → class/status;
  425/503/500/transport/oversize → transient; redaction of ≥ 130-hex runs.
- **Hub projections:** `vp_result.json` → vp 1; `final_hub_check.json` →
  counted for digest `0xd740…`, not for another digest; new recordings below.
- **Expiry:** each rule in §5 at its boundary.
- **Secret wall:** binding, record, marker, read documents: no key containing
  `signature` other than `signature_sha256`, no ≥ 130-hex run;
  `check-route-architecture.sh` still passes (`"secrets"` only in `route/src`).
- **Reads:** `vote_type_supported`/`voting_open` on recorded fixtures.

New recordings before coding (two read-only hub POSTs, saved under
`docs/snapshot-testnet/responses/`): the `vp` and `votes` queries written
**with GraphQL variables**, as the route will send them, for the demo
proposal and the Bloom account `0x0c056ae661D3d3f87919eCb9733527509D392751`.

### Live acceptance (you run it; testnet only)

`V=/petals/ballot/testnet/accounts/main/0/votes`,
`D1='{"vote_id":"demo-1","space":"bloom-ballot-182c.eth","proposal":"0x5fa4f371c47a9691128eaa0bc079abed859f73b9f2e0afb453ff5a7d5b752607","choice":1,"reason":"ballot acceptance"}'`.
Must finish before 2026-11-13T18:21:52Z (demo `end`).

| # | Action | Expected |
| --- | --- | --- |
| 0 | `bash scripts/dev-cycle.sh` | tests, build, check, imports as §7, enroll, (restart), install; three hashes equal |
| 1 | `bloom vfs ls $V`; `bloom vfs cat $V/new.json` | only `new.json`; schema/rules; `last_write: null` |
| 2 | N1 closed: `vote_id:"closed-1"`, proposal `0xb2a3f2fb…25f2`, choice 1 | `-3 refused: proposal_not_active`; `closed-1.json` status `refused`; no ceremony |
| 3 | N2 choice 4 on demo, `vote_id:"bad-choice"` | `-3 refused: choice_out_of_range` |
| 4 | N5 reuse `closed-1` with demo proposal, choice 9 | `-3 refused: choice_out_of_range` (refused id is unbound; not a conflict) |
| 5 | N6 body with an extra field | `-3`; marker `invalid_body`; no record |
| 6 | `bloom vfs write $V/new.json --data "$D1"` | `-3 approval pending (<op>)`; `demo-1.json` `awaiting_approval`, `typed_data`, `digest` D; `/wallets/main/policy-updates/pending/<op>` exists |
| 7 | Owner approves the policy update | ceremony succeeds |
| 8 | same write | `-3 approval pending (<request_id>)`, a new 64-hex id |
| 9 | N4 early retry: same write again before approving | same `<request_id>`; no new ceremony |
| 10 | Run `verify` from `demo-1.json` | `out.hex` = D |
| 11 | Owner opens `/petal-signing-requests/<request_id>.json` → `ceremony_url`; checks `exact_ordered_hashes[0]` = D (no `0x`); approves within ~2 min | approved |
| 12 | same write, immediately | exit 0; `status:"submitted"`, `receipt.id` = D; marker `submitted`; no signature bytes anywhere |
| 13 | `bloom vfs cat $V/demo-1.json` | `live.counted: true`, `hub_vote.id` = D, choice 1 |
| 14 | same write | exit 0; `status:"counted"`; no sign request, no POST |
| 15 | N3 `demo-1` with choice 2 | `-3 … bound to a different vote`; record unchanged |
| 16 | `docs/snapshot-testnet/gql.sh` votes query for demo + account | one vote, id D, choice 1, app `bloom-ballot` |
| 17 | Mount: `echo "$D1" > <mount>$V/new.json`; `cat` `new.json` | write "succeeds"; `last_write.outcome:"counted"` |
| 18 | `bloom vfs ls $V` | `new.json`, `bad-choice.json`, `closed-1.json`, `demo-1.json` |
| 19 | Leak check: `grep -rIoE '[0-9a-fA-F]{130}'` over repo, `artifacts/`, Machine log, every read above | only public `preimage` (132 hex from `1901…`) |
| 20 | mainnet write of `D1` | `-3 refused: mainnet_disabled`, no external call |

## Conflicts with the spec

1. **`vote_id = "new"`** must be reserved (§0.2).
2. **`counted`** uses `hub vote id == digest` and `choice` equal: stricter
   than "this voter's vote with this choice" (a later vote with the same
   choice would otherwise look like ours).
3. **"Identical body"** compared on normalised fields (`reason` absent ≡
   `""`, whitespace and key order ignored); raw sha256 kept in the marker.
4. **Mainnet.** Routes exist under `[network]`, but CLAUDE.md rule 7 forbids
   mainnet votes without authorization: refuse `mainnet_disabled` before any
   external call. Least capability (rule 3) argues for committing the
   `mainnet-sequencer` table only when mainnet is authorized; kept in the
   diff because the spec asks for both.
5. **`signed` is durable**, not just transient, so a crash between signing
   and submission is recoverable.
6. **`expired(approval_expired)`** is inferred from a bare `Denied` plus the
   clock: Bloom hides the denial reason (SKILL.md:515-523).
7. **Mandate (Monday)** slots in after the hub checks and before the binding;
   **superseded (Monday)**: `rejected_by_snapshot(newer_vote_exists)` and the
   read's `superseded_hint` (hub vote for this voter/proposal with a newer
   `created` and a different id) are its inputs.

## Open risks for tomorrow

- `vp`/`votes` queries with variables not yet recorded (argument types).
- Hub `voter` filter case-sensitivity (we send checksummed, as stored,
  `vote.ts:107`).
- Approval-expiry wedge (known-limitations.md §2): mitigated by a new vote_id.
- Broker signature recovery on an identical retry (§0.4) is unverified.
- Host clock vs sequencer: +300 s future limit.
- Every rebuild is a new hash: empty store (SKILL.md:466-470), new enrollment,
  restart and policy ceremony (known-limitations.md §3, §5). Do not rebuild
  while a vote is `awaiting_approval` or `signed`.
- One live ceremony per wallet: finish the policy ceremony before step 8.
- `serde_json` without `preserve_order` sorts keys: harmless for `types`
  (`name` < `type`), covered by the envelope test.
- Mount writes are async: a stale lease blocks a vote_id for up to 120 s.
- The account votes once on the demo proposal; any second vote with a later
  timestamp would replace it (`vote.ts:144-179`). Negative tests above never
  reach signing.

## Verify (implementation day)

CLAUDE.md "Verify" block, plus the §12 recordings, then the human runs
`scripts/dev-cycle.sh` and the live table.
