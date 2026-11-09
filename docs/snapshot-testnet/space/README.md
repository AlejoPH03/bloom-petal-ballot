# Our own Snapshot testnet space (prepared, NOT executed)

Throwaway address (fund this with Sepolia ETH): **`0x042abe4EE65cBC71Bd11297DCea2d69e1650a596`**
Space id (label chosen once, stored in `state/label`): **`bloom-ballot-182c.eth`**

Setup: `export SNAPSHOT_KEY_FILE=/path/to/throwaway.key` (and optionally `SPACE_LABEL=<label>`). Run from this directory, in this order. Each script sources `lib.sh`, which refuses to run
unless the RPC is Sepolia (chainId 11155111) and the sequencer is `https://testnet.seq.snapshot.org/`.

| # | Script | Funds? | What it does |
|---|---|---|---|
| 1 | `01_check.sh` | no | Prints address, balance, label, `isAvailable`, `getRegisterPrice` (MockUSDC) and gas price. |
| 2 | `02_mint_approve.sh` | **Sepolia ETH (2 tx)** | `MockUSDC.mint(self, 2×price)` (open mint, verified by simulation) and `approve(ETHRegistrar, …)`. |
| 3 | `03_commit.sh` | **Sepolia ETH (1 tx)** | ENSv2 `makeCommitment(label, owner, secret, subregistry=0, resolver=0, 90d, referrer=0)` then `commit`. The secret goes to `state/secret` (0600). |
| 4 | `04_register.sh` | **Sepolia ETH (1 tx)** | Waits ≥ 60 s (`MIN_COMMITMENT_AGE`), simulates `register(...)` with `eth_call` first and aborts if it reverts, then sends it. Checks `UniversalHelper.findExactOwner(name) == throwaway`, which is the same check snapshot.js uses for the controller. |
| 5 | `05_create_space.sh` | no (gasless) | Signed `Space` settings message (EIP-712 `Space(address from,string space,uint64 timestamp,string settings)`) → sequencer. It polls the hub until the space appears. |
| 6 | `06_create_proposal.sh` | no (gasless) | ONE proposal: `single-choice`, `["For","Against","Abstain"]`, title `bloom-ballot test`, 7 days, snapshot = latest Sepolia block − 5. It is idempotent. |
| 7 | `07_vote.sh` | no (gasless) | Queries vp, then casts ONE vote (`choice: 1` = For) with the exact snapshot.js `voteTypes`. It polls `votes(where:{proposal, voter})` and prints scores. It is idempotent. |

`08_new_proposal.sh "<title>" "<body>" <minutes> [choices…]` adds a proposal (idempotent per title, `DRYRUN=1` supported).
`09_space_visible.sh` re-signs the space settings with `private:false`, everything else as in 05. `10_demo_delegate_vote.sh` has the throwaway vote For with a reason on "Lower protocol fee to 0.2%" (idempotent). Both support `DRYRUN=1` and keep no signature on disk (`lib.sh` `scrub`). Both ran on 2026-11-09 (FINDINGS.md §12).

`dryrun_test.sh` builds and signs steps 5–7 offline with `DRYRUN=1` (no POST) and cleans up afterwards. It has been run, and it passes.

## Space settings (05)
- strategy `ticket` `{symbol:"VOTE", value:1}` on network `11155111`. Every address has vp 1.
- `filters.onlyMembers: true` and `admins: [throwaway]`, so only the throwaway can propose (`snapshot-sequencer/src/writer/proposal.ts:152-160`).
  Proposal `validation: any`.
- `voteValidation: any`, so any address may vote. vp > 0 is still enforced (`writer/vote.ts:89`).
- `voting.type: single-choice` (enforced on proposals, `writer/proposal.ts:130-131`), `private: true` (hidden from listings; `private: false` since `09_space_visible.sh`, 2026-11-09).

## Sepolia ETH needed
Gas estimates against Sepolia on 2026-11-06 (`cast estimate`): mint 141,730, approve 129,366, commit 128,547.
`register` can't be estimated before a commit exists, so I budgeted ≤ 1,000,000. That makes ≈ 1.4M gas in total.
Gas price at the time was ~20 wei, which makes the real cost negligible, but Sepolia spikes to tens of gwei.
**Fund 0.05 SepoliaETH** (covers 1.4M gas at ~35 gwei). 0.01 is enough at typical prices.
The ENS fee is paid in free MockUSDC (~1.97 USDC for 90 days), so no ETH goes to the fee.

## Unverified until executed
- That `register` accepts `subregistry = 0` and `resolver = 0`. Step 4 simulates before sending, so a revert costs nothing.
  Fallback: pass a PermissionedResolver / PublicResolverV2.
- That the hub picks the space up immediately after step 5 (expected, based on the `settings` writer).
