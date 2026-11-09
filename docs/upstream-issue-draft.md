# Draft issue for bloom-directory/bloom (not filed)

**Before filing:** fetch Bloom and the Petal SDK, and search for
`message_review_payloads` and `bloom.message.review.v1`. The facts below are at
Bloom `433a0daf`. Broker master is `0a9a8da`, last fetched 2026-11-06.

---

## Title

Machine: forward typed data for Petal sign intents so Broker can review it

## Summary

A Petal that signs EIP-712 typed data under its own intent reaches the owner's
ceremony as an opaque hash. Broker master can already render and verify typed
data for Petals. Machine does not send it. Please forward
`message_review_payloads` for Petal exact signing.

## Current behaviour

Reproduced with [`ballot`](https://github.com/AlejoPH03/bloom-petal-ballot),
which signs Snapshot votes with intent `snapshot.vote`:

1. Install a Petal with enrolled provenance on a developer triad, and complete its policy update.
2. Write a vote to `/petals/ballot/testnet/accounts/main/0/votes/new.json` until the signing request is pending.
3. Open the ceremony from `/petal-signing-requests/<id>.json`.

The page says "Sign one operation", names the route, warns that Bloom has not
established the execution effects of these opaque payload digests and hashes,
and lists the exact hash. Space, proposal, choice and reason are not shown.
Today the owner has to recompute the hash outside the ceremony with
`/tools/eip712/hash`.

## Cause

At Bloom `433a0daf`:

- `crates/bloom-machine-client/src/lib.rs:1074-1087` builds `ApprovalPrepareRequest` with only `safe_review_payloads` (empty) and `evm_review_payloads`.
- `crates/bloom-machine-client/src/lib.rs:3094-3106`: `native_evm_review()` returns false for `ProvenanceSubject::Petal`.
- `crates/bloom-daemon/src/lib.rs:2069-2070` reduces the Petal's `action` and `advisory` bytes to sha256 digests.
- `message_review` does not appear anywhere under `crates/`.

In the Petal SDK at `ba76944`, `wit/route/deps/sign-v0.2/sign.wit:25-26` offers
`action` and `advisory`, and `ballot` already passes the complete typed data as
`action` (`route/src/votes/host.rs:181`).

Broker master has the receiving side (`e2f55a7`, "Show owners what they are
signing before they approve", #103):

- `crates/bloom-broker-api/src/service.rs:116`: `ApprovalPrepareRequest.message_review_payloads`.
- `crates/bloom-broker/src/message_review.rs:14,163-194`: envelope `{"schema":"bloom.message.review.v1","kind":"eip712","typed_data":{…}}`.
- `crates/bloom-broker/src/message_review.rs:834-906`: `review()` requires a Petal or system subject, an exact selector and one secp256k1 payload, and refuses unless the sha256 and keccak256 of the preimage it rebuilt equal the selector's digest and hash.
- `crates/bloom-broker/src/message_review.rs:587`: a domain with no chain ID, as in Snapshot's, adds a warning and is accepted.
- `crates/bloom-broker-api/src/approval.rs:37` and `crates/bloom-broker/src/service.rs:826-842`: an envelope is mandatory only for listed classes. `snapshot.vote` is not listed, so it is optional and appears only if Machine sends one.

Bloom's pinned Broker `76bb7e0` does not have this field, so the fix includes
moving the Broker pin.

## Proposal

For Petal exact signing, carry a typed-data review envelope from the Petal to
`message_review_payloads`. Machine need not be trusted with its contents, since
Broker rebuilds the preimage and refuses a mismatch.

Two possible shapes, for the maintainers to choose between:

1. Machine wraps `action` in the envelope when `action` parses as EIP-712 typed data and matches the preimage. No SDK change.
2. A dedicated `message-review` field on `payload-sign-request`. An SDK and WIT change, but `action` keeps its opaque meaning.

## Why it matters

Bloom refuses arbitrary signing, and a named intent is the alternative. That
alternative is only as good as what the owner can see. With this change the
owner reviews the vote's fields in the ceremony: `space`, `proposal`, `choice`
and `reason`, under "Sign typed data", rather than a hash.

Generic review would show `choice: 1` and the proposal id. A line such as
*Vote For "Lower protocol fee" in space X* would need more: the choice label and
title are not part of the signed data, so Broker could not verify them without a
`snapshot.*` review profile.

## Not tested

This change has not been built or run. In particular, `ballot`'s typed data has
not been sent through Broker master's `review()`.
