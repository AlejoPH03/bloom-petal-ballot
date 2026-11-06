# Running the Bloom developer triad on Windows (WSL2)

Bloom documents Linux and macOS only. These notes record a working path on
Windows 11 through WSL2, verified on 4 November 2026.

## Tested combination

| Component | Version |
|---|---|
| Windows | 11, build 26200 |
| WSL | 3.0.3, kernel 6.18 |
| Distro | Ubuntu 24.04 with systemd |
| Rust | 1.99.1 (repo-pinned toolchain) |
| Bloom (Machine) | `433a0daf7ef7457a33a31fb4436c910f6c877bf0` |
| bloom-broker | `76bb7e0956089df44a9b39ca41d1b07497c3ec59` (Bloom's pin) |
| bloom-signer | `3f880d7b96ebcc18f07afadac6f07c7f7036e90c` (Bloom's pin) |
| Chain | Anvil, chain ID 31337 |
| petal (CLI and SDK) | `ba76944a20248493858bfbc2c5f86e3d859f719a` |
| Route contract | `bloom:route@0.1.0`, WIT digest `f52579e1f964eb0302c418f08817593d8f2fc38a434b5f71b483972c56839254` |

## Setup

1. **Install WSL and reboot.** `wsl --install` enables VirtualMachinePlatform,
   and a distro install fails until Windows reboots.
2. **Turn on mirrored networking** before the first boot. This lets a Windows
   browser reach Broker's ceremony page on `localhost:18734` inside WSL.
   In `%USERPROFILE%\.wslconfig`:
   ```ini
   [wsl2]
   networkingMode=mirrored
   memory=10GB
   swap=8GB
   ```
   The memory settings are for a 16 GB host; three Rust workspaces build
   comfortably with them.
3. **Install Ubuntu:** `wsl --install -d Ubuntu-24.04`.
4. **Enable systemd.** The launcher installs per-user systemd socket units.
   Put this in `/etc/wsl.conf`, then run `wsl --shutdown` from PowerShell:
   ```ini
   [boot]
   systemd=true
   ```
   Check with `systemctl --user status`.
5. **Install the tools.** Install `build-essential pkg-config libssl-dev jq git
   curl unzip`, rustup with the `wasm32-unknown-unknown` target, and Foundry.
   The current Foundry installer does not add itself to `PATH`; add
   `export PATH="$HOME/.foundry/bin:$PATH"` to `~/.bashrc`, then run
   `foundryup`.
6. **Work inside the Linux filesystem** (`~/work`), never under `/mnt/c`.

## Pitfalls

- **Broker and Signer must be at Bloom's pinned commits.** With all three
  repositories at the tip of `main`, Machine's requests to Broker fail with
  `UNSUPPORTED_VERSION`, and the launcher stops with `Triad did not pass
  authenticated end-to-end readiness`. Read the pins from Bloom's `Cargo.lock`:
  ```sh
  grep -oE 'bloom-directory/bloom-(broker|signer)[^"]*' Cargo.lock | sort -u
  ```
  Check out those commits in the sibling repositories, rebuild, and start from
  a fresh developer root.
- **Anvil is not configured by default.** The launcher copies
  `~/.bloom/config.toml` into the developer Machine home only once. Add the
  `[chains.anvil]` block from Bloom's `AGENTS.md` to
  `~/.bloom/triad-dev/machine-home/config.toml`, then restart the launcher.
  Keep Anvil running whenever the triad runs.
- **A new wallet can send nowhere.** Its policy starts with
  `allowed_destinations: []`, so a transfer is denied when it is staged. Add the
  recipient through a policy-update ceremony first. A destination entry looks
  like `{"chain":"anvil","destination":"0x…"}`.
- **Approve promptly, then retry at once.** An approval completed late in its
  window was refused by Signer with `APPROVAL_EXPIRED`, about 90 seconds
  before its `expiry_ms`. Polling `approval_challenge.json` and writing
  `retry_body` the moment `state` becomes `active` works reliably.
- **Installing the Petal CLI.** Run `scripts/install-tools.sh` in the `petal`
  repository, then `cargo install --path crates/petal-cli --locked`. The CLI
  crate is in `crates/petal-cli`, not `crates/bloom-petal-cli`.
- **`petal new` pins an SDK version that is not on crates.io.** The scaffold
  asks for `bloom-petal-sdk = "=0.1.0"` in two places, and the build fails with
  `no matching package named bloom-petal-sdk found`. Point both at the same
  git commit of the `petal` repository, as `bloom-petal-hyperliquid` does:
  - in `petal-build.toml`, under `[sdk]`, replace `version` with
    `git = "https://github.com/bloom-directory/petal"` and `rev = "<sha>"`;
  - in `route/Cargo.toml`, change the `petal` dependency to
    `{ package = "bloom-petal-sdk", git = "…", rev = "<sha>" }`.
- **Passkeys work from Windows.** Windows Hello passkeys complete localhost
  ceremonies from Edge or Chrome on the Windows side.
- **Copy with Ctrl+Shift+C** in Windows Terminal. Ctrl+C stops the launcher,
  and with it the whole triad.
- **Ignore the `Failed to translate 'D:\…'` lines** when WSL starts. WSL is
  importing Windows `PATH` entries it cannot map, and they are harmless.

## Verified end to end

- The triad starts with no kernel mount. All access goes through `bloom vfs`.
- A Windows browser reaches the ceremony page.
- Wallet registration completes with a passkey.
- A policy update completes with a passkey.
- An Anvil ETH transfer goes through stage, Deny, policy update, restage,
  approval ceremony and retry, then broadcast. Its receipt shows `outcome:
  success`.
- A Petal scaffolded with `petal new` goes through `petal build`,
  `petal check`, `bloom petals build` and `bloom petals install`, and its route
  is readable through `bloom vfs cat`.
