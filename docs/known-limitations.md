# Known limitations

## 1. The owner's ceremony does not show the vote, only its digest

**At our pins** (Bloom Machine `433a0daf`, Broker `76bb7e0`), Ballot's
`snapshot.vote` signature request reaches the owner's passkey ceremony as an
opaque payload. The page says "Sign one operation", names the requesting Petal
route, warns "Bloom has not established the execution effects of these opaque
payload digests and hashes", and lists the exact hash. Space, proposal, choice
and reason are not shown. See `docs/plan-hello-signature.md` Q1 for the full
trace. In short:

- Machine sends Broker no reviewable payload for a Petal subject.
  `native_evm_review()` is false for `ProvenanceSubject::Petal`
  (`bloom-machine-client/src/lib.rs:1077-1081`, `3094-3106`).
- Petal `action`/`advisory` bytes reach Broker only as sha256 digests inside
  the exact facts (`bloom-daemon/src/lib.rs:2054-2072`).
- Broker at `76bb7e0` has no generic EIP-712 review for Petals, only a
  Safe-specific one (`bloom-broker/src/safe_review.rs`).

### Upstream status (checked 2026-11-06, read-only)

Ran `git -C ~/work/bloom-broker fetch origin`. The default branch is
**`origin/master`** (`refs/remotes/origin/HEAD`); there is no `origin/main`.

- `origin/master` = `0a9a8dae12e86237cb9d6ba63a2456d7c0871cf9` (2026-11-05,
  "Ceremony page fixes from v0.5.0 testing … (#128)").
- Our pin `76bb7e0` is **not** on master. It is on `origin/release/0.4.1`
  (`git branch -r --contains 76bb7e0`). The merge-base is `1a6a8a5`; master
  has 12 commits beyond it, and the pin has 6 release-only commits.

**A generic readable message review exists upstream but not in our pin:**

- Introduced in **`e2f55a7`** "Show owners what they are signing before they
  approve (#103)", 2026-10-23, and extended in **`b6c2733`** (#125, x402/MPP
  payments). Neither commit is in `76bb7e0`.
- File `crates/bloom-broker/src/message_review.rs` (origin/master), schema
  `bloom.message.review.v1`.
- New request field `ApprovalPrepareRequest.message_review_payloads:
  Vec<Base64UrlBytes>`, "Full typed-data/personal-message envelopes,
  independently rebuilt and bound to the exact payload by Broker before owner
  review" (`bloom-broker-api/src/service.rs:113-116` on origin/master).
- Envelope (`message_review.rs:163-194`):
  `{"schema":"bloom.message.review.v1","kind":"eip712","typed_data":{"domain":…,"types":…,"primaryType":…,"message":…}}`.
  The typed data must have exactly those four keys, and the domain only
  `name`/`version`/`chainId`/`verifyingContract`/`salt` (`:550-584`).
- Broker parses the typed data with alloy, **rebuilds the preimage itself**
  (`0x1901 ‖ domain.separator() ‖ hash_struct()`), and renders domain and
  message fields (`:584-640`).
  - It refuses the review unless `sha256(preimage)` and `keccak256(preimage)`
    equal the exact selector's payload digest and hash (`review()`, `:834-906`).
  - It requires a Petal (or system x402/MPP) subject, an exact selector, one
    payload and the `secp256k1-keccak256-recoverable` suite (`:842-871`).
- **Snapshot's domain is accepted.** A domain with no `chainId` only adds the
  warning "The signing domain contains no chain ID; this signature is not
  bound to one network." (`:586-588`).
- Broker *requires* an envelope only for classes in
  `MESSAGE_REVIEW_OPERATION_CLASSES` (`bloom-broker-api/src/approval.rs:37-45`:
  gasless, venice-x402, hyperliquid user actions) and Polymarket's classes
  (`bloom-broker/src/service.rs:825-860` on origin/master). `snapshot.vote` is
  not listed, so for us the envelope would be optional, and the review appears
  only if Machine sends one.
- Class-specific checks (`validate_petal_profile`, `:918+`) exist for
  Polymarket, Hyperliquid, gasless and venice. There are none for `snapshot.*`.

### What Machine would need

- **Machine must populate `message_review_payloads`** for a Petal exact
  request with the envelope above. At `433a0daf` the field does not exist in
  Machine's copy of the Broker API. The exact-signing path builds
  `ApprovalPrepareRequest` with only `evm_review_payloads`/`safe_review_payloads`
  (`bloom-machine-client/src/lib.rs:1074-1087`).
- **Machine must get the typed data from the Petal.** The only Petal-supplied
  bytes on the sign request are `action` and `advisory` (SDK `lib.rs:119-132`,
  WIT `sign-v0.2/sign.wit:16`), and today Machine reduces them to digests.
  Something in Machine (and possibly the Petal SDK/WIT) has to carry the
  envelope through.
- **Not checked:** whether current Bloom upstream already does this. The
  local `~/work/bloom` `origin/master` ref equals our pin `433a0daf` and was
  not fetched for this check (only the Broker fetch was authorized). Next
  step: fetch Bloom and the Petal SDK and search for `message_review_payloads`
  / `bloom.message.review.v1`.
- **Then:** move the Bloom, Broker and Signer pins together, with Ballot
  passing the envelope (the `typed_data` it already builds) the way the
  upgraded SDK specifies.

### Our Step 1 mitigation

`hello-sign.json` is a **pipeline proof**. Its read exposes, all public:

- the full EIP-712 `typed_data` JSON, the 66-byte `preimage` and the
  `claimed_hash`;
- a `verify` section with the exact commands to recompute the digest with
  **Bloom's own** tool, `/tools/eip712/hash`
  (`bloom-vfs/src/handlers/tools.rs:17`, `539-545`), which hashes with alloy
  `TypedData::eip712_signing_hash`.

`verify` holds two commands:

```sh
bloom vfs cat /petals/ballot/<net>/accounts/<wallet>/<n>/hello-sign.json | jq -c .typed_data | bloom vfs write /tools/eip712/hash/ballot-hello/in.json
bloom vfs cat /tools/eip712/hash/ballot-hello/out.hex
```

The owner reads the vote fields in `typed_data`, and Bloom's tool, not
Ballot's code, computes their digest. Before approving, the owner compares
`out.hex` with the exact hash on the ceremony page ("The exact plan that was
reviewed" → `exact_ordered_hashes`, shown without `0x`). This binds what the
owner read to what Signer will sign, but it is a manual step outside the
ceremony. Until Machine supplies message review, every real vote needs this
check.

## 2. An expired exact approval may wedge the identical retry

Once Machine stores an approval id for an exact request, a retry replays it
(`bloom-vfs/src/exact_signing.rs:484-490`, `520-530`). After that approval
expires, nothing in this path clears it, which contradicts SKILL.md:544-545.
This is unverified. The temporary hello route accepts `{"attempt": n}`: a new
`n` is a new claim nonce, so it gets a new request id for the same payload.

## 3. Every rebuild is a new package hash

Each new hash needs provenance re-enrollment, a triad restart and an owner
policy-update ceremony before it can sign (SKILL.md, "Run it in Bloom").
`scripts/dev-cycle.sh` handles the first two. Machine opens the policy
ceremony on the first signing attempt.

## 4. Signer expiry skew

On this triad, Signer refused an approval with `APPROVAL_EXPIRED` about 90 s
before its `expiry_ms`. Approve promptly, and retry immediately after
approving.
