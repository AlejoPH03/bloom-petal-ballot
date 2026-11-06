# CLAUDE.md — rules for Claude Code sessions in this repo

`ballot` is a Bloom Petal: Snapshot proposal reads plus owner-approved,
gasless EIP-712 votes via `bloom:sign` with intent `snapshot.vote` (no outbox).

## Rules

1. **Read `.agents/skills/bloom-petal-development/SKILL.md` first**, then
   `AGENTS.md`, `README.md`, `petal-build.toml`, `petal.toml` and the route
   files in scope. The skill is authoritative for Petal practice.
2. **One handler per route file; no path-based dispatch.** Each file under
   `route/files/` uses one `petal::route_file!` shape. Never branch on
   `current_route_path`, `current_route_canonical_path`, filenames, suffixes
   or parameter presence. `route/src` holds typed, tested helpers only.
3. **Narrowest spec, least capability.** Start from `static_*_spec()` and
   widen only when a route's compiled imports require it. Keep
   `[caps].allowed`, `[[net.allow]]`, `[sign]` and `[store]` in `petal.toml`
   to exactly what routes use; widen deliberately in its own commit.
4. **Never invent SDK APIs, macros, manifest keys or CLI flags.** Find them in
   the pinned SDK source (`~/.cargo/git/checkouts/petal-*/ba76944/`, rev
   `ba76944a20248493858bfbc2c5f86e3d859f719a`) or in reference Petals
   (`~/work/bloom-petal-{hyperliquid,gasless,polymarket}`), and cite them.
   Reference Petals may pin a different SDK rev; this repo's pin wins.
5. **Never write to Bloom `/wallets`, `/petals/*/` (reads only), outbox or
   policy paths.** The only Bloom mutations allowed are
   `bloom petals build .` and `bloom petals install .` for this package.
   Never use `--force`.
6. **No secrets or signature bytes in readable files.** Signatures, keys and
   tokens live only in the store's secret namespace; reads expose status,
   intent, outcome and non-secret identifiers.
7. **No real funds, no mainnet votes** without the owner's explicit
   authorization for that action.
8. **Run Verify before claiming done**, and report failures as they are.

## Verify

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
petal build --root . && petal check --root .
find petal/ballot -name '*.wasm' | while read -r f; do echo "== $f"; wasm-tools component wit "$f" | grep -E '^\s*import' || true; done
source /tmp/bloom-triad-logs/triad.env
bloom petals build . && bloom petals install .
bloom vfs ls /petals/ballot
bloom vfs cat /petals/ballot/status.json
bloom vfs ls /petals/ballot/testnet
bloom vfs ls /petals/ballot/nonsense   # must fail: not found
```
