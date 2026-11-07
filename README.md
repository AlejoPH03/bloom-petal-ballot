# ballot

A Bloom Petal that lets an agent read Snapshot (DAO governance) proposals and
cast gasless EIP-712 votes that Bloom signs only after the owner approves.

**Work in progress.** Proposal reads work on both networks:
`<network>/spaces/<space>/proposals.json` lists a space's active and recent
proposals, and `<network>/proposals/<id>.json` returns one proposal, both
read-only from the Snapshot hub's GraphQL endpoint. Voting is not wired up
yet: the only signing route is the temporary `hello-sign.json` pipeline
proof, and nothing is sent to a Snapshot sequencer. The Snapshot testnet kit
(findings, golden vector, recorded responses, space scripts) is in
`docs/snapshot-testnet/`.

Build and validate:

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
petal build --root . && petal check --root .
bloom petals build .
```

See `AGENTS.md` for the route contract and `docs/` for development notes.
