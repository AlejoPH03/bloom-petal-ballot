# Research: Bloom signing API and route shapes for `ballot`

Read-only research for the `snapshot.vote` signing route. No code in this
repo depends on it yet. Researched 2026-11-06.

**Sources and revisions.** Line numbers refer to these exact checkouts:

| Source | Revision | Local path |
| --- | --- | --- |
| Petal SDK/CLI (**our pin**) | `ba76944a20248493858bfbc2c5f86e3d859f719a` (2026-11-02) | `~/work/petal` (HEAD), also `~/.cargo/git/checkouts/petal-c092b35600511bdf/ba76944` |
| bloom-petal-gasless | `73ccf05`, SDK pin `b9fc22d6d8211bc41304b38b1ef8b5269c8035bd` (2026-07-22) | `~/work/bloom-petal-gasless` |
| bloom-petal-polymarket | `1141037`, SDK pin `4fefa736df67fa1ee51aeaa0c749d2f11414257d` (2026-10-20) | `~/work/bloom-petal-polymarket` |
| bloom-petal-hyperliquid | `b91901b`, SDK pin `864a80b407387871bae06aabe77b91865e55f7bc` (2026-08-26) | `~/work/bloom-petal-hyperliquid` |
| Bloom (Machine) | `433a0daf` | `~/work/bloom` |

All three reference pins are ancestors of `ba76944` (`git merge-base --is-ancestor`).

## TL;DR

1. **Do not copy gasless's signing code.** It calls `petal::sdk::sign_hash`
   with `SignRequest`/`SignHashOutcome::ApprovalRequired`. That API exists at
   its pin (b9fc22d) but **not at ba76944**. It was replaced by
   `sign_payload` plus the Petal use claim in 864a80b ("Agent/triad signing api
   (#21)"). Copy **polymarket** instead: its signing API is identical to our
   pin's apart from internal error mapping and doc comments.
2. The signing call at our pin is `petal::petal_use_claim(..)` →
   `PetalClaim::sign_request(wallet, SignSelector::Exact)` →
   `petal::sdk::sign_payload(&PayloadSignRequest) -> Result<SignOutcome, SdkError>`,
   where `SignOutcome` is `Signature(Vec<u8>)` or `ApprovalPending { action_id, expires_ms }`.
3. **Route shape (6c):** `[network]/accounts/[wallet]/[index]/votes/[vote_id].json`
   **is allowed**. The "suffixed capture" refusal applies only to captures named
   `wallet` or `index`. `.../votes/[vote_id]/record.json` is not needed.
4. **vfs.read (6d):** yes. `WalletAccount::address_evm()` does a host
   `vfs_read`, and Bloom refuses that without `bloom:vfs.read`.

---

## 6a. How the reference Petals request an EIP-712 signature

### gasless (old API, pin b9fc22d), for context only

- **Route file:** `route/files/transactions/[wallet]/[id].json.rs`
  - Spec, lines 2–7:
    `petal::signing_write_spec("gasless.relay").caps(&["bloom:http","bloom:store","bloom:sign","bloom:vfs.read"])`.
    It narrows the broad `write_spec` set by dropping `tx.outbox`, `chain` and `vfs.write`.
  - The read handler (8–30) and write handler (31–64) are both local. The write
    caps the body at 16 KiB (32), resolves the address via
    `crate::wallet_address(&wallet)` (55), and calls
    `crate::gasless_transaction(..)` (63).
  - **The shape is now refused:** `[wallet]` is followed by `[id].json`, not
    `[index]`. See 6c.
- **Address read:** `route/src/lib.rs:12-20` reads `wallets/{wallet}/address`
  with `petal::sdk::vfs_read`. This is the pre-account path layout; at our pin
  it is `wallets/<w>/<n>/address.evm` via `WalletAccount`.
- **Typed data → hash:** `route/src/common.rs:202-233` `signing_hash()` builds
  an `alloy_dyn_abi::eip712::TypedData` from the Relay quote (`types`,
  `primaryType`, `domain`, `message`) and calls `eip712_signing_hash()`. The
  host receives **only the 32-byte hash**: no typed data and no review envelope.
- **Sign call:** `route/src/relay.rs:845-849`:
  `host.sign_hash(&SignRequest { wallet, hash32, purpose: "gasless.relay".into() })`.
  The production adapter is `common.rs:67-69` → `petal::sdk::sign_hash(request)`.
- **SDK definitions at b9fc22d** (`git show b9fc22d:crates/petal-sdk/src/lib.rs`):
  `SignRequest { wallet, hash32: [u8;32], purpose }` at line 81;
  `SignHashOutcome::{Signature(Vec<u8>), ApprovalRequired{action_id, ceremony_url, expires_ms}}`
  at 88; `sdk::sign_hash` at 208.
- **ApprovalPending detection and record:** `relay.rs:854-874`. On
  `ApprovalRequired` it sets `state.phase = "approval_required"` and stores
  `{action_id, ceremony_url, expires_ms, retry_write_body}` in `state.approval`.
  It saves to the **public** store key `state/relay-transactions/{wallet}/{id}.json`
  (`relay.rs:87-89`, `105-109`, written with `store_put(.., secret=false)` at
  `common.rs:47-49`), then returns `-2` (`denied`). Bloom drops the message of a
  `-2`, so the agent must read the file.
- **Identical retry:** the second write finds the existing state
  (`relay.rs:778-793`), re-checks the body against the stored request
  (`validate_request_constraints`), short-circuits when `phase == "submitted"`,
  and recomputes the hash from the **stored** `state.sign`, so the payload is
  byte-identical.
- **Signature storage:** none. The signature lives only in memory. It is
  submitted at once in the URL query (`common.rs:125-149`), the HTTP error is
  discarded so the replayable URL cannot leak, and state moves through
  `submitting` → `submitted` or `submission_unknown` (`relay.rs:886-907`).
  Readable state (`relay.rs:1097-1110`) contains no signature, and a test asserts
  this (`relay.rs:1372`, `1516`).

### polymarket (current API, pin 4fefa73 ≈ our pin)

- **Route files** (all `signing_write_spec(..)` with narrowed `.caps`):
  - `route/files/trade/[wallet]/[index]/drafts/[id]/post.rs:1`:
    `signing_write_spec("polymarket.order.poly1271").caps(&["bloom:http","bloom:store","bloom:sign","bloom:chain","bloom:vfs.read"])`.
    The read handler at 2–4 returns instructions. The write at 5–16 calls
    `petal::wallet_account(ctx)` (6), `petal::wallet_param(ctx)` (7) and
    `petal::param(ctx,"id")` (11), then
    `trade_flow_parts::posting::post_trade_draft(..)`.
  - `route/files/onboard/[wallet]/[index]/begin.rs:1`:
    `signing_write_spec("polymarket.clob_auth.v1").caps(&["bloom:http","bloom:store","bloom:sign","bloom:vfs.read"])`.
  - Also `onboard/.../continue.rs`, `revoke-approvals/.../request/confirm.rs`,
    `withdraw/.../pusd/confirm.rs` and `redeem/.../[slug]/confirm.rs`.
- **EIP-712 preimage:** `route/src/polymarket/eip712.rs:16-22`:
  `signing_preimage(value: &impl SolStruct, domain) = 0x19 0x01 ‖ domain.hash_struct() ‖ value.eip712_hash_struct()`.
  Bloom receives the full 66-byte **preimage** plus `claimed_hash = keccak256(preimage)`.
  `approval.rs:45-55` re-verifies `keccak256(preimage) == hash` before signing.
- **Prepared record:** `approval.rs:10-18` `PreparedSigning { operation, intent, owner, signing_hash, signing_preimage_hex, preimage: Value }`.
  It is persisted once (`store_prepared_signing`, `approval.rs:285-294`, public
  store) and reloaded on retry (`load_prepared_signing`, 296–305). Example:
  `trade_flow_parts/posting.rs:164-166`.
- **Sign call**, `approval.rs:104-219` `sign_prepared_items(..)`:
  1. `PayloadSignItem { preimage, claimed_hash }` (116–124).
  2. `petal::petal_use_claim(ctx, operation_class, SUITE, &payloads, effects, petal::ClaimNonce::Derived)`
     (136–143), with `SUITE = CryptoSuite::Secp256k1Keccak256Recoverable` (8).
  3. `claim.batch_request(wallet, selector)` (144). Then `request.approval_hint`
     (145) and `request.action = Some(prepared_bytes)` (146). **`action` is the
     review payload**: the serialized persisted `PreparedSigning`, which is
     byte-identical across retries. No `advisory` is set.
  4. `petal::sdk::sign_payload_batch(&request)` (147). Single-payload callers
     are `sign_prepared` (333–352, `SignSelector::Exact`) and
     `sign_prepared_reusable` (359–378).
  5. Each returned signature is recovered and checked against the prepared
     owner (151–169).
- **ApprovalPending:** `approval.rs:177-200`. On
  `SignBatchOutcome::ApprovalPending { action_id, expires_ms }` it writes the
  artifact `{action_id, expires_ms, prepared_artifact_digest, retry_state:"approval_required", operation, selector, request_count}`
  to `approval_key` (**public** store, `secret=false`), then returns **`-3`**
  rather than `-2` so the message reaches the agent (comment at 221). The
  artifact is readable through a route, e.g.
  `route/files/onboard/[wallet]/[index]/approval.json.rs:3`
  (`petal::read_store(..)`). Approval keys: `posting.rs:165`
  (`{base}/approval.json`), `onboarding/mod.rs:16-18`.
- **Identical retry:** `ClaimNonce::Derived` (the nonce depends only on package
  hash, route, class and payload digest; SDK `claim.rs:435-446`). Together with
  the same persisted `action` bytes, this means a retry presents the same claim.
  On success the approval artifact is deleted (170).
- ⚠ **Approval-hint divergence.** Polymarket passes the stored pending
  `action_id` back as `approval_hint` (`existing_approval_hint`, `approval.rs:410-434`).
  When the host rejects it, polymarket retires it (201–215). SKILL.md
  ("Owner approval pending", lines 546–547) says: *"Do not pass the pending
  action id back as `approval_hint`; a hint other than the derived request id
  is refused."* **For ballot, leave `approval_hint: None`** (the default from
  `PetalClaim::sign_request`).
- **Signature storage:** when a signature must outlive the write, it goes to
  the secret namespace. Example: `infra_parts/relayer.rs:369` uses key
  `creds/onboard/{wallet}/prepared_relayer_signature.json`, and 398–405 writes
  it with `store_put_json(.., true)`. API credentials use the same pattern
  (`onboarding/mod.rs:146`, `account_views.rs:236`). The CLOB order signature is
  used immediately and not stored (`posting.rs:283-292`).
- **Readable state:** `approval.json` (action id, expiry, digest and selector,
  but no signature), `review_intent.json`, receipts and status. Route files
  never touch secrets, and the scaffold's `scripts/check-route-architecture.sh`
  enforces this.

### Pinned SDK (ba76944) signing surface: what ballot will use

`crates/petal-sdk/src/lib.rs`:

| Item | Line |
| --- | --- |
| `pub use claim::{ClaimAsset, …, CryptoSuite, PetalClaim, petal_use_claim}` | 22–26 |
| `enum SignSelector { Exact, Reusable }` | 87 |
| `struct PayloadSignRequest { wallet, preimage, claimed_hash: [u8;32], signature_algorithm, operation_class, petal_use_claim_jcs, claim_assurance_evidence, approval_hint, action, advisory, selector, key_ref_jcs }` | 119–132 |
| `struct PayloadSignItem { preimage, claimed_hash }` | 135 |
| `struct PayloadBatchSignRequest` | 146 |
| `enum SignOutcome { Signature(Vec<u8>), ApprovalPending { action_id, expires_ms } }` (**no `ceremony_url`**; the owner sees it at `/petal-signing-requests/<id>.json`, per SKILL.md 534–536) | 226–229 |
| `enum SignBatchOutcome` | 232 |
| `sdk::sign_payload(&PayloadSignRequest) -> Result<SignOutcome, SdkError>` | 443 |
| `sdk::sign_payload_batch` | 469 |
| `sdk::store_put(key, value, secret: bool)` | 592 |
| `namespace_for_key`: `secret==true` or a `creds/` prefix → `"secrets"`, otherwise `"state"` | 677–683 (consts 402–403) |
| `const CAPS_HTTP_STORE_SIGN_VFS` (broad write caps) | 1221 |
| `write_spec()` / `signing_write_spec(intent)` | 1272 / 1279 |

`crates/petal-sdk/src/claim.rs`: `CryptoSuite` 73 (`Secp256k1Keccak256Recoverable`
→ `"secp256k1-keccak256-recoverable"`, 93–95); `ClaimEffects::moves_nothing()`
216; `moves_value()` 235; `ClaimNonce { Derived, Explicit([u8;16]) }` 435–446;
`petal_use_claim(ctx, class, suite, &payloads, &effects, nonce)` 581;
`PetalClaim::sign_request(wallet, selector) -> Result<PayloadSignRequest, _>` 634
(sets `approval_hint/action/advisory = None`); `batch_request` 664.

WIT: `wit/route/deps/sign-v0.2/sign.wit:3` (`interface signing`), 4
(`record approval-pending`), 16 (`record payload-sign-request`), 58
(`sign-payload`), 59 (`sign-payload-batch`).

**Differences from gasless:** gasless sends a bare hash with a `purpose`
string. Our pin sends `preimage` + `claimed_hash` + `signature_algorithm` +
`operation_class` + a JCS `petal_use_claim`, plus optional `action`/`advisory`
bytes for review. The outcome is `ApprovalPending` (no `ceremony_url`), not
`ApprovalRequired`. Hyperliquid (864a80b) already uses `sign_payload` but
builds `PayloadSignRequest` by hand (`route/src/workflow.rs:171-217`,
`ApprovalPending` at 341). At our pin, use the claim builder.

**Implied ballot recipe (for the next milestone, not implemented):** build the
Snapshot `Vote` EIP-712 preimage (`0x1901 ‖ domainSep ‖ structHash`) once,
fixing its `timestamp`. Persist it as the operation record. Then:
`petal_use_claim(ctx, "snapshot.vote", Secp256k1Keccak256Recoverable, &[item], &ClaimEffects::moves_nothing(), ClaimNonce::Derived)`
→ `sign_request(account.wallet(), SignSelector::Exact)` → set `action` to the
persisted record bytes → `sdk::sign_payload`. Record `ApprovalPending` in a
readable record and return `-3`. Keep the signature only in memory, or under a
`creds/` key / `secret=true`, until the sequencer accepts it. The exact class
and effects that Broker accepts for `snapshot.vote` are not verified here.

## 6b. Manifest sections, verbatim

**gasless** `petal.toml`:

```toml
[caps]
allowed = ["bloom:http", "bloom:store", "bloom:sign", "bloom:vfs.read"]

[[net.allow]]
binding = "relay"
host = "api.relay.link"
methods = ["get", "post"]
paths = ["/quote/v2", "/execute/permits", "/intents/status/v3"]

[sign]
allowed_intents = ["gasless.relay"]

[store]
namespaces = ["state"]
```

(No `secret_namespaces`, which is consistent with never storing the signature.)

**polymarket** `petal.toml` (signing-relevant parts; it has six more
`[[net.allow]]` tables for gamma/data/clob/enso/relayer):

```toml
[caps]
allowed = ["bloom:http", "bloom:store", "bloom:sign", "bloom:tx.outbox", "bloom:chain", "bloom:vfs.read", "bloom:vfs.write"]

[[net.allow]]
binding = "clob"
host = "clob.polymarket.com"
methods = ["post"]
paths = [
  "/auth/api-key",
  "/auth/builder-api-key",
  "/balance-allowance/update",
  "/order",
]

[sign]
allowed_intents = [
  "polymarket.clob_auth.v1",
  "polymarket.onboard",
  "polymarket.order.poly1271",
  "polymarket.relayer_batch",
]

[store]
namespaces = ["state"]
secret_namespaces = ["secrets"]
shared_keys = ["secrets/creds/enso-api-key", "state/settings/enso-router"]
```

**hyperliquid** `petal.toml`:

```toml
[caps]
allowed = ["bloom:http", "bloom:store", "bloom:sign", "bloom:key.derive"]

[[net.allow]]
binding = "mainnet"
host = "api.hyperliquid.xyz"
methods = ["post"]
paths = ["/info", "/exchange"]

[[net.allow]]
binding = "testnet"
host = "api.hyperliquid-testnet.xyz"
methods = ["post"]
paths = ["/info", "/exchange"]

[sign]
allowed_intents = ["hyperliquid.approve_agent", "hyperliquid.agent_action", "hyperliquid.usd_send", "hyperliquid.usd_class_transfer", "hyperliquid.withdraw", "hyperliquid.order", "hyperliquid.cancel", "hyperliquid.cancel_by_cloid", "hyperliquid.schedule_cancel", "hyperliquid.update_leverage"]

[store]
namespaces = ["state"]
secret_namespaces = ["secrets"] # approval reconciliation only; never key material
```

**Likely ballot shape later** (not applied): `[caps].allowed = ["bloom:http","bloom:store","bloom:sign","bloom:vfs.read"]`;
`[sign] allowed_intents = ["snapshot.vote"]`; `[store] namespaces = ["state"]`
(plus `secret_namespaces = ["secrets"]` only if a signature is persisted);
`[[net.allow]]` per host with explicit methods and paths: `hub.snapshot.org`
`post` `["/graphql"]` and `seq.snapshot.org` `post` `["/"]`, plus the testnet
equivalents once step0 verifies the hosts. Remember the SKILL.md rule (lines
307–308): a path without a trailing `*` matches only itself.

## 6c. Route shapes: is `.../[wallet]/[index]/votes/[vote_id].json` allowed?

**SKILL.md's exact rule** (`.agents/skills/bloom-petal-development/SKILL.md:95-104`):

> `[wallet]` and `[index]` are reserved for the route's own Bloom wallet and
> numbered account, and Bloom grants wallet authority only to the exact
> adjacent segments `<feature>/[wallet]/[index]/...`, for example
> `intents/[wallet]/[index]/new`, resolving both against its own wallet
> projection before the route runs. It refuses any other shape before the
> Petal runs: a `[wallet]` not immediately followed by `[index]`, such as
> `[wallet]/[account]`, an `[index]` not immediately preceded by `[wallet]`,
> and a suffixed capture, so `[wallet]/[index].json.rs` is refused too.
> `petal build` and `petal check` warn about such a shape but still succeed,
> and `bloom petals build` accepts it.

The troubleshooting section (SKILL.md:525-527) puts it the same way: *"a
suffixed capture such as `[index].json`"*.

**Reading of "suffixed capture":** it refers to a suffixed `[wallet]` or
`[index]` only. Evidence:

- **Petal builder (our pin)** `crates/petal-builder/src/lib.rs:839-842`
  `capture_name()` and `844-893` `route_shape_violations()`: the suffix
  violation fires only for `Some("wallet") | Some("index") if *segment != wallet && *segment != index`.
  The test at `1285-1295` asserts `balance/[wallet].json` and
  `balance/[wallet]/[index].json` are flagged, and that
  `keys/[wallet_id].json` is **not**.
- **Bloom Machine** `crates/bloom-petals/src/router.rs:451-460`: account
  dispatch requires only that the pattern contain the adjacent pair
  `["[wallet]", "[index]"]` (`pattern.windows(2)`). Captures elsewhere in the
  pattern are not inspected.
- **Bloom matches suffixed captures generally:** `crates/bloom-petals/src/runner.rs:1206-1216`
  `route_segment_matches()` strips the literal suffix (`.json`) and binds a
  non-empty remainder.
- **Shipped precedent:** hyperliquid v0.1.7 (installed on this triad) has
  `route/files/[network]/exchange/[wallet]/[index]/withdrawals/[nonce].json.rs`,
  a suffixed non-account capture under `[wallet]/[index]`. Polymarket has
  none, and gasless's `transactions/[wallet]/[id].json.rs` is invalid for a
  different reason: no `[index]`.

**Conclusion:** `[network]/accounts/[wallet]/[index]/votes/[vote_id].json` is
allowed. `[network]` before the feature directory matches hyperliquid's
`[network]/exchange/[wallet]/[index]/...`. A `.../votes/[vote_id]/record.json`
layout is not required, though it remains an option if a vote later needs
sibling files such as `receipt.json`. Also needed:
`[network]/accounts/$index.rs` and `[network]/accounts/[wallet]/$index.rs`
(the latter is `petal::account_listing!()`). `vote_id` must still be validated
in the route (`validate_vote_id`), because the `.json` suffix is stripped
before binding.

## 6d. Listing wallets and reading the account address

- **Hyperliquid does not use `petal::account_listing!()` or
  `petal::wallet_account(ctx)`.** Neither exists at its pin 864a80b. Both were
  added in petal 4fefa73, "feat(sdk): resolve the wallet account from trusted
  context (#42)", 2026-10-20 (verified with `git show <rev>:crates/petal-sdk/src/lib.rs | grep`).
  Hyperliquid writes the equivalent by hand:
  - `route/files/[network]/exchange/$index.rs:2` and
    `route/files/[network]/exchange/[wallet]/$index.rs:2`:
    `petal::route_file!(spec: petal::static_dir_spec(), list: Vec::new());`
    with the comment *"Bloom supplies live wallet/index directory inventory
    from the core projection."*
  - `[network]/exchange/[wallet]/[index]/$index.rs:1-17`: a static child list.
  - Routes read `petal::param(ctx, "wallet")` and parse it with the Petal's own
    `crate::parse_wallet_id` (e.g. `[index]/last_response.json.rs:8-13`).
    It never reads the account address: the owner address is recovered from
    signatures (README).
- **Bloom fills these listings itself:** `crates/bloom-petals/src/router.rs:387-427`
  `discovered_entries()` lists `wallets` for a `[wallet]` position and
  `wallets/<w>` for an `[index]` position. `router.rs:465-482` never runs the
  guest for a `[wallet]` directory with `[wallet]/[index]` routes beneath it.
- **At our pin:** `petal::account_listing!()` (`lib.rs:1419-1424`) expands to
  exactly `route_file!(spec: static_dir_spec(), list: Vec::new())`.
  Polymarket still writes it by hand (`route/files/trade/[wallet]/$index.rs:2`).
- **`petal::wallet_account(ctx)`** (`lib.rs:1460-1487`) reads the trusted
  `bloom.wallet`/`bloom.account` params. It cross-checks the `[wallet]` capture
  and returns `-2` when the route is not account-mounted. Methods: `wallet()`,
  `number()`, `dir()` (`wallets/<w>/<n>`), `path(rest)`,
  `address_evm_path()` (1527), and `address_evm()` (1537) →
  `address_evm_from(&mut HostVfs)` (1548) → `read_address` (1592) →
  `HostVfs::read` → **`sdk::vfs_read`** (1579–1585, 622). Polymarket uses it
  at `route/src/infra_parts/host_calls.rs:9-27`.
- **Does `address_evm()` need `bloom:vfs.read`? Yes.** Bloom's component host
  `crates/bloom-petals/src/vm.rs:2283-2290` `component_vfs_read` checks
  `caps.contains(&Capability::VfsRead)`. Without it, it logs
  `host capability denied` (`vm.rs:2877-2884`) and refuses. There is no
  exemption for the account's own `address.evm` leaf. Every reference route
  that reads the address declares it: polymarket `post.rs:1` and `begin.rs:1`,
  gasless `[id].json.rs:6`, and the SDK's `wallet_http_read_spec` and
  `account_read_spec` (`lib.rs:1255-1263`). It must also be in
  `[caps].allowed`. `wallet_account(ctx)` itself makes no host call and needs
  no capability.

## 6e. Pinned SDK vs reference pins: summary of differences

| | gasless (b9fc22d) | hyperliquid (864a80b) | polymarket (4fefa73) | **ours (ba76944)** |
| --- | --- | --- | --- | --- |
| Sign function | `sdk::sign_hash(&SignRequest)` | `sdk::sign_payload` (hand-built request) | `sdk::sign_payload_batch` via `PetalClaim` | `sdk::sign_payload` / `_batch`, `PetalClaim` builder |
| Pending outcome | `ApprovalRequired{action_id, ceremony_url, expires_ms}` | `ApprovalPending{action_id, expires_ms}` | same | same |
| Payload | 32-byte hash + `purpose` | preimage + claimed hash + claim | same | same |
| `wallet_account` / `account_listing!` | — | — | ✓ | ✓ |
| Account path | `wallets/<w>/address` | n/a | `wallets/<w>/<n>/address.evm` | same |

Between 4fefa73 and ba76944, `git diff` on `crates/petal-sdk/src/{lib,claim}.rs`
changes only the internal error mapping (`map_err(host_err)` → `.host()`, plus
`sdk::last_host_reason()` at `lib.rs:721`) and claim doc comments about
Broker destination matching. No public signing type or signature changed.
