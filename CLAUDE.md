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
   policy paths.** Never write any `/wallets/*/policy.json`: Machine proposes
   the package-hash policy update itself on the first signing attempt, and
   the owner approves it. Never use `--force`.
   - Installer-provenance enrollment
     (`bloom init triad-enroll-developer-petal-provenance`) is allowed **only
     inside `scripts/dev-cycle.sh`**, which the human runs. It pauses for the
     human to restart the triad.
   - **Claude Code never restarts the triad, never runs `scripts/dev-cycle.sh`,
     and never completes or cancels ceremonies** (policy-update or signing).
   - The only other Bloom mutations allowed are `bloom petals build .` and
     `bloom petals install .` for this package, and only when asked.
   - For changes that do not need signing, Claude Code may run
     `bloom petals uninstall ballot` (never `--force`) followed by
     `bloom petals build . && bloom petals install .`.
   - `scripts/dev-cycle.sh` (enrollment + triad restart) is only for signing
     tests, and only the human runs it.
6. **No secrets or signature bytes in readable files, logs, test output or
   reports.** Signatures, keys and tokens live only in the store's secret
   namespace, or nowhere. Reads expose status, intent, outcome, non-secret
   identifiers and at most a sha256 fingerprint of a signature. Never pass
   `approval_hint`.
7. **No real funds, no mainnet votes** without the owner's explicit
   authorization for that action.
8. **Run Verify before claiming done**, and report failures as they are.

## Verify

Host-side (Claude Code runs these):

```sh
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh
petal build --root . && petal check --root .
bash scripts/leak-check.sh
find petal/ballot -name '*.wasm' | sort | while read -r f; do echo "== $f"; wasm-tools component wit "$f" | grep -E 'import bloom:' || true; done
bash -n scripts/dev-cycle.sh
```

Bloom-side, signing builds: the **human** runs `bash scripts/dev-cycle.sh`
(enroll → restart triad → build → install → hash check). For a build that
signs nothing, `bloom petals build .` and `bloom petals install .` are
allowed when asked. Read-only checks:

```sh
source /tmp/bloom-triad-logs/triad.env
bloom vfs ls /petals/ballot
bloom vfs cat /petals/ballot/status.json
bloom vfs ls /petals/ballot/testnet
bloom vfs ls /petals/ballot/nonsense   # must fail: not found
```
