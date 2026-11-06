# ballot

A Bloom Petal that lets an agent read Snapshot (DAO governance) proposals and
cast gasless EIP-712 votes that Bloom signs only after the owner approves.

**Work in progress.** This milestone ships only a static skeleton
(`status.json` and the `testnet/` and `mainnet/` network directories). It
makes no network requests and does not sign.

Build and validate:

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
petal build --root . && petal check --root .
bloom petals build .
```

See `AGENTS.md` for the route contract and `docs/` for development notes.
