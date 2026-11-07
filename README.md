# ballot

A Bloom Petal that lets an agent read Snapshot (DAO governance) proposals and
cast gasless EIP-712 votes that Bloom signs only after the owner approves.

**Status.** Proposal reads work on both networks:
`<network>/spaces/<space>/proposals.json` lists a space's active and recent
proposals, and `<network>/proposals/<id>.json` returns one proposal (with
`vote_type_supported` and `voting_open`), read-only from the Snapshot hub.
Votes (`<network>/accounts/<wallet>/<n>/votes/new.json`, one record per vote
in `votes/<vote_id>.json`) are implemented and host-tested on **testnet**
(docs/plan-votes.md); the live acceptance run with owner ceremonies is
pending. Mainnet votes are disabled. The Snapshot testnet kit (findings,
golden vector, recorded responses, space scripts) is in
`docs/snapshot-testnet/`.

At these Bloom pins the owner's ceremony shows only the vote's digest, not
its fields: before approving, run the `verify` commands in
`votes/<vote_id>.json` (docs/known-limitations.md §1).

Build and validate:

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
petal build --root . && petal check --root .
bloom petals build .
```

See `AGENTS.md` for the route contract and the agent's vote procedure, and
`docs/` for development notes.

## Enabling mainnet votes

Mainnet votes are refused by design (`mainnet_disabled`, before any external
call): a mainnet vote spends real governance power, and this build is tested
on testnet only. An owner who wants mainnet votes makes two deliberate
changes, in their own commit:

1. Add the mainnet sequencer to `petal.toml`:

   ```toml
   [[net.allow]]
   binding = "mainnet-sequencer"
   host = "seq.snapshot.org"
   methods = ["post"]
   paths = ["/"]
   ```

2. Remove the `Network::Mainnet` gate at the top of `draft()` in
   `route/src/votes/flow.rs` (and its test
   `mainnet_is_refused_before_any_external_call`).

Both change the package hash, so the new build needs installer provenance,
a triad restart and the owner's policy-update ceremony
(`scripts/dev-cycle.sh`) before it can sign.
