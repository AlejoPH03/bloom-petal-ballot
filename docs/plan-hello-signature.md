# Plan: Step 1, "hello signature"

**Status: DECIDED and IMPLEMENTED (2026-11-06).** The lead reviewed the plan;
the decisions are below. The code follows them. `scripts/dev-cycle.sh` is
ready but has **not** been run: the human runs it.

Goal: the smallest route that requests **one** `snapshot.vote` signature over a
**fixed** test Snapshot Vote, and proves four things:

1. installer-provenance enrollment works;
2. the owner's policy-update ceremony allows the package hash;
3. the owner completes a passkey ceremony for the vote;
4. an identical retry returns a signature.

Nothing is submitted to Snapshot, no funds are involved, and signature bytes
never reach a readable file, log or report.

## Lead decisions (2026-11-06)

1. **Step 1 is a PIPELINE proof.** At these pins the ceremony shows only the
   digest. Mitigation: the `hello-sign.json` read exposes the full public
   typed-data JSON, the `claimed_hash`, and the exact commands to recompute
   the digest with Bloom's **own** `/tools/eip712/hash`
   (`bloom-vfs/src/handlers/tools.rs:17`, `539-545`). The owner compares it
   with the digest on the ceremony screen without trusting Ballot's code.
   Upstream status: `docs/known-limitations.md`. Broker master already has a
   generic EIP-712 message review (`e2f55a7`, #103), but it is not in our pin
   and Machine does not feed it.
2. **Provenance enrollment is allowed ONLY inside `scripts/dev-cycle.sh`.**
   The script pauses for the human to restart the triad. Claude Code never
   restarts the triad, never writes any `/wallets/*/policy.json`, and never
   completes or cancels ceremonies. CLAUDE.md rule 5 says exactly this.
3. **`{"attempt": n}` write body accepted** for the temporary hello route
   only. The same `n` is an identical retry.
4. **alloy-primitives `=1.6.1` (features `std`, `k256`) and k256 `=0.13.4`**,
   the versions bloom-petal-hyperliquid uses (its `route/Cargo.toml` and
   `Cargo.lock`). k256 is a dev-dependency, as in Hyperliquid. Recover the
   signer and compare it with `address_evm()`. Record
   `signature_recovers_to_account` (bool) and a sha256 fingerprint; discard
   the bytes.
5. **No manual policy file.** Rely on Machine's automatic policy-update
   proposal on the first signing attempt (Q6).

## Implementation map

| Piece | File |
| --- | --- |
| Snapshot Vote EIP-712 (domain separator, struct hash, 66-byte preimage, digest, typed-data JSON, checksum, recovery, fingerprint) + golden-vector tests | `route/src/snapshot.rs` |
| Attempt body, record, read document with `verify`, SDK signing chain | `route/src/hello.rs` |
| Routes | `route/files/[network]/{$index,accounts/$index,accounts/[wallet]/$index,accounts/[wallet]/[index]/$index,accounts/[wallet]/[index]/hello-sign.json}.rs` |
| Manifest | `petal.toml` (`[caps]`, `[sign]`, `[store]`) |
| Dev cycle | `scripts/dev-cycle.sh` |

## Revisions cited

| Component | Revision | Path |
| --- | --- | --- |
| Petal SDK (our pin) | `ba76944` | `~/work/petal`, `crates/petal-sdk/src/{lib,claim}.rs` |
| Bloom Machine | `433a0daf` | `~/work/bloom` |
| Bloom Broker | `76bb7e0` (= Bloom's pin, `bloom/Cargo.toml:137`) | `~/work/bloom-broker` |
| bloom-rpc-wire | `48b83c5` (Broker's pin, `bloom-broker/Cargo.toml:18`) | `~/.cargo/git/checkouts/bloom-service-runtime-db2120f8e16caa43/48b83c5` |
| Polymarket (reference) | `1141037` | `~/work/bloom-petal-polymarket` |
| Step 0 | FINDINGS.md 2026-11-06 17:40 + `golden/` 17:48 | `~/work/step0` |

## ⚠ Headline findings that change the brief

1. **The passkey ceremony cannot show readable vote fields at these pins.**
   Broker never sees a Petal's preimage in reviewable form, and Petal
   `action`/`advisory` bytes reach Broker only as sha256 digests (Q1). The
   owner sees "Sign one operation", the requesting route, the claim, an
   opaque-payload warning and the digests. Step 1 can prove the signing
   pipeline; it cannot prove readable review. That needs an upstream change
   to Broker and Machine. See the [options](#q1-options).
2. **The owner policy update is automatic.** On the first signing attempt,
   Machine proposes `current policy + this package hash` and opens the
   policy-update ceremony itself (Q6). The proposed
   `~/policy-proposed-ballot.json` hand-off is unnecessary. A manual
   `policy.json` write would add a competing pending proposal, so I recommend
   dropping it.
3. **Enrollment writes Bloom config** (`~/.bloom/triad-dev/config/provenance-catalog.json`)
   and requires a triad restart. That goes beyond "only `bloom petals build`
   and `bloom petals install`" in CLAUDE.md rule 5, so the lead must authorize
   it explicitly (Q7).
4. **A possible wedge after the 5-minute window.** In the code, an exact
   request whose approval expired keeps replaying that approval id, which
   contradicts SKILL.md:544-545 ("opens a new ceremony"). The design adds an
   escape hatch (Q4).
5. **Vote types:** use `string from` / `string proposal`, as current
   snapshot.js does, not the spec's `address`/`bytes32` (FINDINGS.md:43-58).

---

## Q1. How Broker presents a Petal payload signature to the owner

**Path of a request.** Route → `sdk::sign_payload` (SDK `lib.rs:443`) → Bloom
VM `component_payload_sign_record` (`bloom-petals/src/vm.rs:1700-1730`) →
Machine `sign_payload_outcome` (`bloom-daemon/src/lib.rs:1946`). For
`SignSelector::Exact`, Machine builds `canonical_facts` (`lib.rs:2054-2072`)
and calls `BrokerExactPayloadSigner::sign_or_prepare_petal`
(`bloom-vfs/src/exact_signing.rs:304`). That calls `prepare_approval` in the
Machine client (`bloom-machine-client/src/lib.rs:1074-1087`), which reaches
Broker `service.rs:848 prepare_approval`.

**What Broker gets to review:**

- `ApprovalPrepareRequest` (`bloom-broker-api/src/service.rs:100-128`) has
  `terms`, `canonical_plan_facts_digest`, `evm_review_payloads`,
  `safe_review_payloads`, `petal_use_claim` and `system_use_claim`. It has
  **no field for Petal `action`/`advisory` bytes**.
- Machine sends `evm_review_payloads` only when `native_evm_review()` is true,
  and that is **always false for a `ProvenanceSubject::Petal`**
  (`bloom-machine-client/src/lib.rs:1077-1081`, `3094-3106`). It sends
  `safe_review_payloads: Vec::new()` (`:1076`).
- `action` and `advisory` survive only as `"action_digest"` and
  `"advisory_digest"` (sha256 hex) inside Machine's `canonical_facts`
  (`bloom-daemon/src/lib.rs:2070-2071`). Their JCS digest becomes
  `canonical_plan_facts_digest` (`exact_signing.rs:432-435`). They are
  **bound, not displayed**.
- Broker starts the context as `ReviewManifestContext::default()`
  (`bloom-broker/src/service.rs:859`) and sets only the claims and EVM/Safe
  reviews (`:955-960`, `1001-1003`). `attributed_advisory_items` is never
  filled in production. It is only set in a test (`ceremony.rs:5875`).
- `canonical_review_plan` (`ceremony.rs:5039-5100`) renders asset amounts,
  `terms`, ordered digests/hashes, security disclosures and optional
  `evm_review`/`safe_review`. There are no free-form fields.

**Generic EIP-712 review:** none for Petals. The only typed-data reader is
Safe-specific (`bloom-broker/src/safe_review.rs:57`, `230`), and only for a
package that declares the Safe class (`service.rs:868-905`). Searches for
`bloom.message.review`, `eip712`, `typed_data` and `TypedData` across
`bloom-broker/src` and `bloom-broker-api/src` find nothing else.

**What the owner sees** for our exact claim with no debits, destinations or
fee. From `ceremony_assets/app.js`:

- title "Approve transaction" and button "Approve with passkey" (`:86-90`);
- `describeTransfer`'s claim branch (`:615-652`): sentence "Sign one
  operation on …" with an empty chain, because our claim has none; facts
  `Network`, `Requested by: Petal <route id>`, and `Bloom verification: No —
  these figures are claimed, not verified`;
- warning "Bloom has not established the execution effects of these opaque
  payload digests and hashes." (`ceremony.rs:5193-5201`, shown via
  `app.js:993-998`);
- an expandable "The exact plan that was reviewed" (`app.js:1159-1164`) with
  the terms and the `exact_hashes`. **The `exact_hashes` entry is our EIP-712
  digest**, the one fact the owner can cross-check.

**Can a name+version-only domain be signed?** Yes. No component parses the
preimage for a Petal subject. Machine checks only `claimed_hash ==
suite_hash(preimage)`, i.e. keccak256 for `secp256k1-keccak256-recoverable`
(`bloom-machine-client/src/lib.rs:901-905`). The VM checks that the preimage
is non-empty, the suite is supported and the claim is canonical
(`vm.rs:1731-1742`).

### Q1 options

| Option | Readable fields in ceremony | Cost |
| --- | --- | --- |
| **A. Step 1 as pipeline proof (recommended now).** Show the readable vote and its digest in `hello-sign.json`; the owner cross-checks the digest in the ceremony's "exact plan". | No; digest cross-check only | None upstream |
| B. Upstream: a Broker `snapshot_review`, or a generic Petal EIP-712 review, like `safe_review` (Machine must pass the preimage for a Petal class; Broker decodes it and adds it to `canonical_review_plan`). | Yes, Broker-verified | Broker + Machine PRs; new pins |
| C. Upstream: render Petal `advisory` as `attributed_advisory_items` (the plumbing exists in `ReviewManifestContext`). | Yes, but labelled Petal-asserted, not verified | Machine + Broker PRs |

Recommendation: do **A** for Step 1, state the gap honestly in the README, and
raise **B** with the lead and Bloom as the real fix. C is weaker: the same
Petal that builds the payload would assert the text.

## Q2. Operation class and claim

- **Class string:** `snapshot.vote` is a valid Bloom `Token`: 1-64 bytes, a
  lowercase first byte, then `[a-z0-9._/-]` (`bloom-rpc-wire/src/ids.rs:55-65`;
  SDK `claim.rs:830-851`).
- **Where it must appear:**
  - in the route spec's `sign_intent` (SDK `signing_write_spec`, `lib.rs:1279-1281`);
  - in `[sign].allowed_intents`, which Bloom checks at install
    (`bloom-petals/src/package.rs:510-516`) and at runtime
    (`vm.rs:1727-1731`, `2442-2447`);
  - in the provenance record, which enrollment derives from the route's
    `sign_intent` (`bloom/src/triad_enrollment.rs:524-543`).
- **Effects:** a vote moves no value, so use `petal::ClaimEffects::moves_nothing()`
  (SDK `claim.rs:216-225`): no debits, no destinations, `ClaimFee::None`.
- **Fee asset:** enrollment writes `fee_asset: None` for every developer class
  (`triad_enrollment.rs:540`). Broker then requires `DeclaredFee::None`.
  Anything else is `FEE_NOT_ALLOWED` (`bloom-broker/src/authority.rs:4323-4357`).
  `moves_nothing()` satisfies this.
- **`allowed_destinations` (spec Q2): not consulted for this class.** Broker
  checks only the destinations a claim declares
  (`authority.rs:3218-3230`), and SDK `claim.rs:19-21` says the same. With
  `moves_nothing()` there are none. The current policy lists two Anvil
  destinations; they are irrelevant here.
- **Other claim checks:**
  - Broker requires the claim's package, route, class, suite, payload digest
    and hashes to match the trusted context (`authority.rs:3171-3183`). The
    SDK's `petal_use_claim` fills these from `ctx` (SDK `claim.rs:581-590`).
  - `claim_assurance` is `machine_asserted`. That is acceptable while
    `policy.required_verifiers` is empty (`authority.rs:3189-3217`), and the
    current policy has `"required_verifiers":[]`.
- **Polymarket's "Broker-reviewed classes"** (`approval.rs:70-90`, citing
  Broker #103 `validate_petal_profile`): no such function exists at Broker
  `76bb7e0`, and there is no class-specific profile for any `snapshot.*`
  class.

## Q3. Preimage

Confirmed. Sign the 66-byte `0x19 0x01 ‖ domainSeparator ‖ structHash` with
`claimed_hash = keccak256(preimage)`, exactly as Polymarket does
(`polymarket/route/src/polymarket/eip712.rs:16-22`; its re-check is in
`approval.rs:45-55`). Machine enforces the hash relation
(`bloom-machine-client/src/lib.rs:901-905`).

- Domain: `EIP712Domain(string name,string version)` with
  `{name:"snapshot", version:"0.1.4"}` (FINDINGS.md:28-30).
- Type: `Vote(string from,string space,uint64 timestamp,string proposal,uint32 choice,string reason,string app,string metadata)`
  (FINDINGS.md:31-34).
- **Golden vector** (not yet written up in FINDINGS.md §9, which still says
  "pending"; files in `~/work/step0/golden/`):
  - `message.json`: from `0x042abe4E…a596`, space `bloom-ballot-test.eth`,
    timestamp `1793923200`, proposal `0x3b1261…dcc6`, choice 1, reason `""`,
    app `bloom-ballot`, metadata `{}`.
  - `digest_parts.txt`: typeHash `0x7bf372d7…cdffa`, domainSeparator
    `0x484fce18…18670`, structHash `0x2b0bbfd2…65f08`.
  - Digest `0x9431fb703c91b4a1c216343688a73580e102f80585c4511376f7b58281ff0251`,
    produced independently by `digest_manual.sh` (cast) and
    `digest_bloom.hex`. How `digest_bloom.hex` was produced is not documented
    yet; mark that pending.
- **Is the review material, rather than the bare preimage, what the owner
  sees?** Neither is shown (Q1). The owner sees the digest. `action` bytes are
  bound by digest only.

## Q4. Pending, ceremony and retry

**Two different pendings reach the route as `SignOutcome::ApprovalPending`
(SDK `lib.rs:226-229`):**

| Stage | Machine code | `action_id` is | Owner finds the ceremony at |
| --- | --- | --- | --- |
| Package not yet allowed by the wallet policy | `lib.rs:1994-2005` → `wallets.rs:1903-1977` | the **policy-update operation id** | `/wallets/<w>/policy-updates/pending/<op>/approval_challenge.json` (`ceremony_url`) and `.../status.json` (`wallets.rs:207-212`) |
| Exact signature awaiting the owner | `lib.rs:2112-2150` | the derived **request id** | `/petal-signing-requests/<request_id>.json` (`ceremony_url`, `status:"awaiting_owner_approval"`; `lib.rs:2135-2146`; mounted at `:4694-4699`) |

Both locations are owner-only: guest VFS reads of `petal-signing-requests` are
denied (`lib.rs:527-537`). The route records the `action_id` and returns `-3`
so the text reaches `bloom vfs write`. Polymarket does the same
(`approval.rs:177-200`, `221`), because Bloom drops the message of a `-2`
(SKILL.md:517-521).

**What makes a retry identical:**

- The request id is BLAKE3 over package hash, route id, wallet, operation
  class, `claimed_hash`, **canonical claim JCS** and account key ref
  (`bloom-daemon/src/lib.rs:1043-1076`).
- The claim's `ClaimNonce::Derived` is SHA-256 over domain ‖ package hash ‖
  route ‖ class ‖ payload digest, truncated to 16 bytes (SDK `claim.rs:435-446`,
  `683-700`). So the same payload under the same package gives the same claim
  and the same request id.
- The persisted exact state must also match: payload digest, claimed hash,
  provenance digest and `canonical_plan_facts_digest`, which includes the
  `action`/`advisory` digests. Otherwise the request is refused with "exact
  signing retry differs from its persisted operation identity"
  (`exact_signing.rs:465-475`).
- The signing operation digest also binds `policy_version`/`policy_digest`
  (`bloom-machine-client/src/lib.rs:1003-1017`). Any policy change after the
  exact approval breaks it.
- So the vote `timestamp` must be a constant, the address must come from
  `wallet_account`, and `action`, if set, must be deterministic bytes.
- **Never pass `approval_hint`.** Machine refuses any hint other than the
  derived request id (`lib.rs:2033-2050`). The SDK default is `None`
  (`claim.rs:653`).

**Timing:**

- The exact approval TTL is `5 * 60 * 1000` ms from the first attempt
  (`exact_signing.rs:27`, `457`).
- A local reset happens only if no approval was ever recorded
  (`exact_signing.rs:484-490`).
- Once an approval id is stored, a retry asks Broker for its status. While
  `Prepared`/`AwaitingCeremony` it returns the same pending result
  (`exact_signing.rs:360-399`, `520-528`). Otherwise it tries to sign with
  that approval (`:530`).
- **Risk:** after expiry, nothing in this path clears the stored approval id,
  so the identical retry could be refused indefinitely. SKILL.md:544-545
  claims a new ceremony opens. This is unverified; test it on the triad.
- **Escape hatch:** an optional write body `{"attempt": n}` (0-255; default 0)
  selects `ClaimNonce::Derived` for 0 and `ClaimNonce::Explicit([n; 16])`
  otherwise (SDK `claim.rs:444-446`). That changes the canonical claim and
  therefore the request id, without changing the payload or the package hash.
- **CEREMONY_RATE_LIMITED:** one live ceremony per wallet
  (`bloom-broker/src/ceremony.rs:3170-3181`; SKILL.md:548-559). Finish the
  policy ceremony before the exact one; Machine does this anyway because
  eligibility runs first on every attempt. Don't trigger other signing on
  `main` meanwhile.
- **APPROVAL_EXPIRED early:** Signer refused about 90 s before `expiry_ms` on
  this triad. Procedure: open the URL at once, approve within about 2
  minutes, then retry immediately.
- **Restarts:** at startup Machine invalidates every pending Petal ceremony
  projection (`ceremony_unavailable_after_restart`, `lib.rs:257`,
  `3936-3990`). Never restart the triad between a pending write and its
  approval.

## Q5. Capabilities, spec and manifest

**Route spec** (one file, read + write):

```rust
petal::route_file!(
    spec: petal::signing_write_spec("snapshot.vote")
        .caps(&["bloom:sign", "bloom:store", "bloom:vfs.read"]),
    read: |ctx: &petal::Ctx| { /* last attempt record */ },
    write: |ctx: &petal::Ctx, body: &[u8]| { /* … */ },
);
```

- `signing_write_spec(intent)` = `write_spec()` + `sign_intent`
  (SDK `lib.rs:1272-1281`). It sets `write_async` and the route metadata's
  `sign_intent` (`lib.rs:1297-1331`).
- `.caps(..)` (`lib.rs:1189`) narrows the broad default set
  (`lib.rs:1221-1229`) to what the component imports:
  - `bloom:sign/signing` → `bloom:sign`;
  - `bloom:vfs/readwrite` (read only, via `WalletAccount::address_evm()` →
    `HostVfs` → `sdk::vfs_read`; SDK `lib.rs:1537`, `1579-1585`, `622`) →
    `bloom:vfs.read`, checked at `vm.rs:2283-2290`;
  - `bloom:store/kv` → `bloom:store`, for the attempt record;
  - `bloom:env/runtime` needs no capability.
- No `bloom:vfs.write` (it would fail at runtime, SKILL.md:250-262), no
  `bloom:http`, no `bloom:tx.outbox`, no `bloom:chain`.
- After `petal build`, verify the compiled imports with `wasm-tools component wit`.

**Directory routes** stay `static_dir_spec()` with no capabilities.

**`petal.toml` diff:**

```diff
 [caps]
-allowed = []
+allowed = ["bloom:sign", "bloom:store", "bloom:vfs.read"]
+
+[sign]
+allowed_intents = ["snapshot.vote"]
+
+[store]
+namespaces = ["state"]
```

- `[sign]` with the `bloom:sign` cap is required at install
  (`package.rs:3906`, `2231`).
- `[store] namespaces = ["state"]` uses Hyperliquid's and Polymarket's form.
  There is no `secret_namespaces`, because no secret is stored.
- No `[[net.allow]]`: nothing contacts the network.

**Shared crate dependencies:** keccak256 plus ecrecover, to prove the
signature recovers to `address.evm` before discarding it. Use
`alloy-primitives = { version = "=1.6.1", default-features = false, features = ["std", "k256"] }`,
Hyperliquid's exact pin (`bloom-petal-hyperliquid/route/Cargo.toml`).
Polymarket uses the heavier `alloy = "2"` (`route/Cargo.toml:16`). Add
`sha2 = "0.10"` for the signature fingerprint.

## Q6. Policy entry format and package hash

- **Format:** `allowed_petal_packages: Vec<Digest32>`
  (`bloom-broker-api/src/policy.rs:14`). `Digest32` is
  `fixed_lower_hex!(Digest32, 32, …)` (`bloom-rpc-wire/src/ids.rs:134`,
  macro `:67-105`). It is a `#[serde(transparent)]` JSON **string of exactly
  64 lowercase hex characters, with no `0x` prefix**. Example:
  `"allowed_petal_packages":["e9f2d9abc93627736417dfff446b6d9a64147d5ab90d07b3bcfe4e2b2e3c521c"]`.
- **Current policy** (read-only, `bloom vfs cat /wallets/main/policy.json`):
  `{"allowed_destinations":[…2 anvil entries…],"allowed_petal_packages":[],"maximum_approval_lifetime_ms":2592000000,"required_verifiers":[],"wallet_id":"main"}`.
- **Machine adds the hash itself.** `ensure_petal_eligibility`
  (`bloom-vfs/src/handlers/wallets.rs:1903-1977`):
  1. resumes any pending proposals (`:1915-1931`);
  2. reads the Broker policy and verifies its digest and canonical bytes
     (`:1932-1951`);
  3. returns `Allowed` if the hash is present (`:1952-1954`);
  4. otherwise stages `policy_with_package(&policy, hash)`, i.e. the current
     policy plus this hash (`bloom-machine-client/src/petal_eligibility.rs:11-20`),
     as a policy update (`wallets.rs:1955-1977`).
- **Exact hash to allow:** the hash Bloom runs the route under,
  `context.package_hash` (`lib.rs:1992`). That equals the `hash:` printed by
  `bloom petals build .` and `bloom petals install .` for the same tree, and
  the prefix shown by `bloom petals ls`.
- **Recommendation:** no `~/policy-proposed-ballot.json` and no manual
  `policy.json` write. The owner approves the ceremony Machine opens.
- **If the lead still wants a manual path,** generate the file only: the
  current JSON with the 64-hex hash appended, all else preserved. Do it only
  while no policy update is pending, because Machine refuses to rebase over
  unrelated pending consent (`wallets.rs:1913-1914`).
- **Each new build hash needs another enrollment, restart and policy
  ceremony.** Enrollment marks the previous developer package in the lineage
  inactive (`triad_enrollment.rs:432-460`), and the policy allows by exact
  hash.

## Q7. Provenance enrollment

- **Command:**
  `bloom init triad-enroll-developer-petal-provenance ~/.bloom/triad-dev/config ~/work/bloom-petal-ballot`.
  - Arguments `config_dir petal_dir` (`bloom/src/main.rs:2536-2541`).
  - Hidden, `triad-dev-harness` feature only; `$BLOOM_BIN` from `triad.env`
    is that build.
  - Linux is allowed, root refused (`triad_enrollment.rs:163-171`).
  - Config dir from `triad.env` (`BLOOM_PROVENANCE_CATALOG=…/config/provenance-catalog.json`).
- **What it does** (`triad_enrollment.rs:357-521`):
  - runs `build_petal_package_dir(petal_dir)`, which regenerates
    `artifacts/` the way `bloom petals build` does, and takes its hash (`:378-381`);
  - for every route whose metadata has a `sign_intent`, writes a
    `ProvenanceRecord{subject: Petal{package_hash, route: route_id}, operation_classes:[{snapshot.vote, fee_asset:None}]}`
    (`:476-497`);
  - re-signs the catalog and rewrites `provenance-catalog.json` (`:499-519`).
  - **It fails if no route declares a class** (`:493-498`).
- **Order** (SKILL.md:508-513: enrollment hashes the components already built
  under `petal/<name>/`; Machine and Broker read the catalog at startup):
  1. `petal build --root . && petal check --root .`
  2. Enroll (command above). **Touch nothing in the tree afterwards.**
  3. Restart the triad: Ctrl+C the launcher and rerun it
     (`docs/dev-setup-wsl.md:88`). The launcher re-signs the catalog and keeps
     developer records (`bloom/scripts/triad-dev-launch.sh:358-366`).
  4. `source /tmp/bloom-triad-logs/triad.env`, then `bloom petals build .`
     and `bloom petals install .`.
- **Confirm the enrolled hash equals the installed hash** with three values
  that must be equal:
  - `H_install`: the `hash:` line from `bloom petals install .`;
  - `bloom petals ls | grep petals/ballot/` (12-hex prefix);
  - `jq -r --arg h "$H_install" 'any(.records[]; .subject.kind=="petal" and .subject.package_hash==$h)' ~/.bloom/triad-dev/config/provenance-catalog.json`
    must print `true`. This mirrors the launcher's own check at
    `triad-dev-launch.sh:367-372`.
- **Failure signs:**
  - missing provenance: Machine WARN `petal.sign_payload_denied` with
    `reason = installer provenance does not authorize snapshot.vote`
    (`exact_signing.rs:428`, logged at `bloom-daemon/src/lib.rs:2095-2110`;
    SKILL.md:471-475);
  - the route sees a bare refusal.

---

## Proposed design (adjusted)

### Routes

| Route | Kind | Spec / caps | Behaviour |
| --- | --- | --- | --- |
| `[network]/$index` | dir | `static_dir_spec()` | validates network; lists `accounts/` |
| `[network]/accounts/$index` | dir | `static_dir_spec()`, `list: Vec::new()` | Bloom lists wallets (`bloom-petals/src/router.rs:387-427`), as Hyperliquid's `exchange/$index.rs:2` does |
| `[network]/accounts/[wallet]/$index` | dir | `petal::account_listing!()` (SDK `lib.rs:1419-1424`) | never runs; Bloom lists accounts (`router.rs:465-482`) |
| `[network]/accounts/[wallet]/[index]/$index` | dir | `static_dir_spec()` | `vec![petal::writable("hello-sign.json")]` |
| `[network]/accounts/[wallet]/[index]/hello-sign.json` | writable, **temporary** | as in Q5 | see below |

The shape `<feature>/[wallet]/[index]/<leaf>` is valid
(`petal-builder/src/lib.rs:844-893`; `router.rs:451-460`). `petal build`
must print no `warning: route`.

### `hello-sign.json`

**Write** (use `bloom vfs write`, which is synchronous and returns errors;
the mount runs it async, SKILL.md:340-346):

1. Body ≤ 64 bytes: empty, or `{"attempt": n}` with `n` 0-255. Otherwise `-3`.
2. Network from `[network]` via `crate::Network::parse`; unknown → `-1`.
3. `account = petal::wallet_account(ctx)` (SDK `lib.rs:1460`). Then
   `from = account.address_evm()` (`:1537`), keeping Bloom's checksum casing
   as snapshot.js does (FINDINGS.md:38).
4. The fixed vote, all constants in `route/src`:
   - space `bloom-ballot-hello.eth`;
   - proposal `"0x0000…0001"`, 66 chars, the `string` type;
   - choice `1`, reason `"hello from ballot"`, app `"bloom-ballot"`,
     metadata `"{}"`;
   - timestamp `HELLO_TIMESTAMP = 1793923200` (the golden constant;
     irrelevant here since nothing is submitted).
5. `preimage = 0x1901 ‖ domainSeparator ‖ structHash`;
   `claimed_hash = keccak256(preimage)`.
6. `claim = petal::petal_use_claim(ctx, "snapshot.vote", CryptoSuite::Secp256k1Keccak256Recoverable, &[PayloadSignItem{preimage, claimed_hash}], &ClaimEffects::moves_nothing(), nonce)`
   (SDK `claim.rs:581`), with `nonce` = `Derived`, or `Explicit` for
   attempt > 0.
7. `req = claim.sign_request(account.wallet(), SignSelector::Exact)?`
   (`claim.rs:634-662`). Set `req.action = Some(JCS of the readable vote)`:
   deterministic, bound by digest but not displayed. `approval_hint`,
   `advisory` and `key_ref_jcs` stay `None`.
8. `petal::sdk::sign_payload(&req)` (`lib.rs:443`):
   - `Signature(bytes)`: check the length is 65, recover against
     `claimed_hash`, and require recovered == `from`. Store
     `{status:"signed", signature_sha256, recovered_address}`. **Drop the
     bytes.** Return `Write`.
   - `ApprovalPending{action_id, expires_ms}`: store
     `{status:"awaiting_approval", action_id, expires_ms}`. Return `-3` with
     the text "approval pending (<action_id>); the owner opens the ceremony;
     retry the exact same write".
   - `Err`: store `{status:"error", message}` with the message sanitized, and
     no `SdkError` text that could carry payload. Return `-3` for Denied and
     `-4` otherwise.

**Read:** the last attempt record for this network, wallet and account
(store key `hello/<network>/<wallet>/<n>.json`, `state` namespace via `store_put(.., secret=false)`):

- `schema`, `status` (`not_attempted` | `awaiting_approval` | `signed` | `error`);
- `attempt`, `action_id`, `expires_ms`;
- the readable `vote` object, `claimed_hash` (0x hex), `signer_address`;
- `signature_sha256` (hex, only after `signed`), `updated_ms`.

It never holds signature bytes and contains nothing else. When status is
`awaiting_approval` and `action_id` is not a 64-hex request id, the record
adds a hint to look under `/wallets/<w>/policy-updates/pending/` (the policy
stage).

### Shared crate (`route/src`), host-tested

- `snapshot::vote_preimage(&Vote) -> [u8; 66]` and `vote_digest()`, hand-rolled
  over keccak: `typeHash`, string fields keccak'd, `uint64`/`uint32`
  left-padded into 32 bytes.
- Golden test: `golden/message.json` → `digest_parts.txt` values and digest
  `0x9431fb70…0251`. Plus negative tests: changed field order, the
  `address`/`bytes32` legacy types, and an altered domain.
- `hello::Record` serde type (`deny_unknown_fields`), with a test that it
  has no signature field and no 130-hex run.
- `snapshot::recover_signer(digest, sig) -> Result<Address, _>`, tested two
  ways. (1) Hermetically, with a signature made at test time by Hyperliquid's
  deterministic `k256` test key. (2) Against the golden throwaway signature,
  **read at test time** from `~/work/step0/golden/signature.hex` (override
  with `BALLOT_GOLDEN_SIGNATURE`), so no signature bytes are committed. The
  test skips with a message if the file is absent. The throwaway private key
  is never used.

### `scripts/dev-cycle.sh` (revised)

1. `cargo test`, `petal build`, `petal check`, and the import listing.
2. Print the enrollment command and require typed confirmation. It writes
   Bloom config; lead approval is needed (headline 3).
3. Enroll.
4. "Restart the triad now (Ctrl+C the launcher and rerun it), then press
   Enter". Then `source triad.env`.
5. `bloom petals build .`, then `bloom petals install .` (never `--force`),
   capturing `H_install`.
6. Verify the three-way hash equality (Q7). Abort if they differ.
7. Print the owner steps below.

It never writes `/wallets/*`. It does **not** generate a policy file.

### Test procedure, with expected output

`P=/petals/ballot/testnet/accounts/main/0/hello-sign.json`

| # | Action | Expected |
| --- | --- | --- |
| 0 | `bloom vfs cat $P` | `"status":"not_attempted"`, plus `typed_data`, `preimage`, `claimed_hash`, `verify` |
| 1 | `bloom vfs write $P --data '{"attempt":0}'` | Error text containing `approval pending (<op>)`. Record `status:"awaiting_approval"`, `action_id` = **policy op id**. `bloom vfs ls /wallets/main/policy-updates/pending` shows `<op>` |
| 2 | Owner: `bloom vfs cat /wallets/main/policy-updates/pending/<op>/approval_challenge.json` → open `ceremony_url` → "Approve policy change" (`app.js:951-955`), diff adds the package | Ceremony succeeds |
| 3 | `bloom vfs write $P --data '{"attempt":0}'` (identical) | Machine commits the policy (`wallets.rs:1915-1931`) and the exact approval is prepared. Error text with `approval pending (<request_id>)`, a 64-hex id. Record has a new `action_id` and `expires_ms` about now + 5 min. `/wallets/main/policy.json` now lists the hash |
| 3b | Run the two `verify` commands from `bloom vfs cat $P` (they write only to Bloom's `/tools/eip712/hash/ballot-hello/` scratch session) | `out.hex` = record `claimed_hash` |
| 4 | Owner: `bloom vfs cat /petal-signing-requests/<request_id>.json` → `status:"awaiting_owner_approval"`, open `ceremony_url` **promptly** | Page "Approve transaction" / "Sign one operation", "Requested by Petal <route id>", opaque-payload warning. **Cross-check:** "exact plan" `exact_ordered_hashes[0]` = `out.hex` from 3b (without `0x`). Approve within about 2 minutes |
| 5 | `bloom vfs write $P --data '{"attempt":0}'` **immediately** | Success (exit 0). Record `status:"signed"`, `signature_recovers_to_account: true`, `signer_address` = checksummed `bloom vfs cat /wallets/main/0/address.evm`, `signature_sha256` present. `/petal-signing-requests/<id>.json` shows `"status":"signed"` |
| 6 | `bloom vfs write $P --data '{"attempt":0}'` again | Expected (unverified): signs again under the same exact identity, or refuses as spent. Record the observed behaviour |
| 7 | Leak check: `grep -rIoE '[0-9a-fA-F]{130}'` over the repo, `artifacts/`, the Machine log and `bloom vfs cat $P`. Ignore the public `preimage` (132 hex starting `1901…`) | No other matches |

**If step 5 is refused with an expired approval:** record the Machine log line
`petal exact signing failed`, then rerun with `--data '{"attempt":1}'`. That is a new
request id and a new exact ceremony; the policy stays approved.

**Logs:** `grep -a 'petal.sign_payload_denied\|petal exact signing failed' <log-dir>/machine.log`
and Broker `rpc.completed` (SKILL.md:594-624).

### Cleanup after Step 1

Delete `hello-sign.json.rs` and its listing entry. Keep `snapshot::vote_*`
and its tests. Revisit `[caps]`: Step 2 adds `bloom:http` and
`[[net.allow]]`. Each rebuild means a new hash, so enroll, restart and
approve the policy again.

## Contradictions with the spec or the research file

1. **Spec:** "passkey ceremony that shows readable vote fields". Not possible
   at Broker `76bb7e0` / Machine `433a0daf` (Q1). Needs upstream option B.
2. **Spec:** dev-cycle writes `~/policy-proposed-ballot.json`. Unnecessary,
   because Machine proposes the policy itself (Q6). Dropped.
3. **Spec:** "write body ignored". Changed to an optional `{"attempt":n}`
   escape hatch (Q4).
4. **Spec types `address`/`bytes32`.** Use `string`/`string`
   (FINDINGS.md:43-58).
5. **research-signing.md** said the owner reaches every pending via
   `/petal-signing-requests/<id>.json`. That is true only for the exact stage;
   the first pending is a **policy** operation under
   `/wallets/<w>/policy-updates/pending/` (Q4).
6. **research-signing.md** treated retry identity as payload + claim. It also
   binds `canonical_plan_facts_digest` (including action/advisory digests),
   the provenance digest and the policy version/digest (Q4).
7. **research-signing.md / Polymarket on `approval_hint`.** On the exact path
   the returned `action_id` *is* the request id, so Polymarket's hint is
   accepted there. It is refused when the stored id was a policy op id
   (`lib.rs:2033-2050`). Our rule, never pass a hint, is unchanged.
8. **SKILL.md:544-545** ("five minutes after the first attempt opens a new
   ceremony") does not match `exact_signing.rs:484-490` once an approval id is
   stored. Unverified; covered by the attempt nonce.
9. **CLAUDE.md rule 5** (only `bloom petals build`/`install` may change Bloom)
   must be amended to allow developer provenance enrollment and triad restarts
   for this milestone.

## Open questions for the lead (resolved)

All four were answered on 2026-11-06; see "Lead decisions" at the top.
