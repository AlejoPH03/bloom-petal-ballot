# ballot

A Bloom Petal that lets an agent read Snapshot (DAO governance) proposals and
cast gasless EIP-712 votes that Bloom signs only after the owner approves.

**Status.** Proposal reads work on both networks:
`<network>/spaces/<space>/proposals.json` lists a space's active and recent
proposals, and `<network>/proposals/<id>.json` returns one proposal (with
`vote_type_supported` and `voting_open`), read-only from the Snapshot hub.
Votes (`<network>/accounts/<wallet>/<n>/votes/new.json`, one record per vote
in `votes/<vote_id>.json`) work on **testnet** and passed the live
acceptance run with owner ceremonies (docs/plan-votes.md). Mainnet votes are disabled. The Snapshot testnet kit (findings,
golden vector, recorded responses, space scripts) is in
`docs/snapshot-testnet/`.

`<network>/accounts/<wallet>/<n>/inbox.json` is the agent's starting point
(open proposals in the mandate's spaces, whether this account voted,
closing-soon and result alerts), and `mandate.json` holds the owner's
standing rules (allowed spaces, blocked keywords, required reason, a daily
limit, delegates to follow). `following.json` offers the open proposals a
followed delegate voted on and this account has not, each with a ready
`votes/new.json` body; nothing votes automatically. **The mandate is advisory:** the Petal stores it and the agent can
edit it. It filters bad drafts before the owner is asked; it is not a
security boundary, and every vote still needs the owner's approval in a
passkey ceremony. An owner-signed, binding mandate is future work
(docs/plan-mandate-inbox.md).

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

## Installing

Every install path below gives all the read routes: `status.json`, proposals,
`inbox.json`, `following.json` and `mandate.json`. **Signing is separate.**
`votes/new.json` also needs installer provenance for that exact package hash,
and the wallet owner's approval of the package ("Signing votes" below).

If a different build of `ballot` is installed, run `bloom petals uninstall
ballot` first. Never use `--force`.

### From the release archive (the path for reviewers)

```sh
gh release download v0.1.0-rc1 -R AlejoPH03/bloom-petal-ballot
sha256sum -c ballot-v0.1.0-rc1.petal.tar.gz.sha256
bloom petals install ./ballot-v0.1.0-rc1.petal.tar.gz
```

Bloom does not build anything for this path. The archive is built by
`petal package` from a clean checkout of the tag. Release page:
<https://github.com/AlejoPH03/bloom-petal-ballot/releases/tag/v0.1.0-rc1>.

### From a local checkout

```sh
bash scripts/build.sh
bloom petals build . && bloom petals install .
```

Bloom hashes every file under the root except `.git` and `target`, including
git-ignored files such as `docs/snapshot-testnet/space/state/` or a `dist/`
directory, so remove what should not ship.

### From GitHub source

```sh
bloom petals install https://github.com/AlejoPH03/bloom-petal-ballot --ref v0.1.0-rc1
```

**This is refused at our Bloom pin:** `unsupported GitHub owner AlejoPH03;
trusted owner is bloom-directory`. Bloom accepts source installs only from the
`bloom-directory` organization (docs/known-limitations.md §9). The package is
ready for it: `petal.toml` has `[source]` and `[build]` like the reference
Petals, and `scripts/build.sh` works in a fresh checkout. If the repository
moves there, set `[source].repository` to the new name.

A source install gives all the read routes. Bloom rebuilds it in a fresh
temporary checkout, so its hash differs from any local build and from one
install to the next. It therefore cannot be enrolled for signing in advance.

### Signing votes

A signing route works only when Bloom's installer provenance catalog lists
that exact package hash for the `snapshot.vote` operation class. Without it,
reads work and `votes/new.json` is refused with a bare permission error. The
first signing attempt then opens a policy-update ceremony, and the wallet
owner approves the package hash.

On a developer triad, the owner enrolls the package themselves (the command
exists only in the `triad-dev-harness` build of `bloom`). Enroll and install
the **same directory**:

```sh
mkdir ballot-pkg && tar -xzf ballot-v0.1.0-rc1.petal.tar.gz -C ballot-pkg
"$BLOOM_BIN" init triad-enroll-developer-petal-provenance "$BLOOM_TRIAD_DEVELOPER_ROOT/config" ballot-pkg
# restart the triad, then:
bloom petals uninstall ballot   # only if another build is installed
bloom petals install ballot-pkg
```

Do not enroll the extracted tree and then install the `.petal.tar.gz`. The
hashes differ, because enrollment and directory installs include the generated
`artifacts/` and a `petal package` archive does not
(docs/known-limitations.md §10). For a checkout, `bash scripts/dev-cycle.sh`
does all of this.

On a packaged Bloom, an owner cannot enroll a package: its catalog has to pin
the hash, as it does for the Petals Bloom ships. That hash is the one
`bloom petals install <archive>` reports, which is also `package_hash` in a
release's `petal-release.json`.

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
