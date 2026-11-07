# Create your own Snapshot testnet space (bloom-ballot step 0)

Outcome of step 0 of the Bloom `ballot` petal: Snapshot testnet facts, a golden EIP-712 Vote vector, and scripts to
stand up a private Snapshot **testnet** space (Sepolia ENSv2 name + ticket strategy) where any fresh address can vote.
Every protocol fact, with source citations and recorded responses, is in [`FINDINGS.md`](FINDINGS.md).

## Requirements
`cast` (Foundry), `curl`, `jq`, `python3`; a **throwaway** key with ~0.05 Sepolia ETH (only the 4 ENS txs cost gas).

## Setup
```bash
cast wallet new                       # copy ONLY the private key into a file outside the repo
chmod 600 /path/to/throwaway.key
export SNAPSHOT_KEY_FILE=/path/to/throwaway.key
export SPACE_LABEL=my-test-label      # optional; otherwise a random "bloom-ballot-xxxx" label is stored in space/state/
# optional: ETH_RPC_URL (default publicnode Sepolia), SNAPSHOT_JS_DIR (checkout of snapshot.js to check type hashes)
```

## Run (see `space/README.md` for details)
| step | script | gas? |
|---|---|---|
| 1 | `space/01_check.sh` – address, balance, availability, price | no |
| 2 | `space/02_mint_approve.sh` – free MockUSDC + approve ENSv2 ETHRegistrar | yes |
| 3 | `space/03_commit.sh` – ENSv2 commit | yes |
| 4 | `space/04_register.sh` – register (simulated first), verify owner | yes |
| 5 | `space/05_create_space.sh` – signed Snapshot `settings` | no |
| 6 | `space/06_create_proposal.sh` – one "bloom-ballot test" proposal | no |
| 7 | `space/07_vote.sh` – one vote + hub confirmation | no |
| + | `space/08_new_proposal.sh "<title>" "<body>" <minutes>` – extra proposals, idempotent per title | no |

`space/dryrun_test.sh` signs steps 5–7 offline (`DRYRUN=1`, nothing is POSTed).
Scripts hard-refuse any RPC other than Sepolia and any sequencer other than `https://testnet.seq.snapshot.org/`.
`space/state/` holds the ENS commit secret and run outputs — keep it out of git.

## Other contents
- `gql.sh '<query>'` – POST a GraphQL query to `https://testnet.hub.snapshot.org/graphql`.
- `gqlv.sh <query.graphql> '<variables>'` – POST a query file with GraphQL variables (the read routes' request shape).
- `golden/` – golden Vote vector for the Rust unit test (`typed_data.json`, digest, signature, signer) and the
  independent digest script (`digest_manual.sh`, cast-only hashStruct).
- `responses/` – raw hub responses cited in FINDINGS.md.

## In this repository
This folder is the sanitized step 0 kit, copied into `bloom-petal-ballot` as `docs/snapshot-testnet/`.
`space/.gitignore` keeps `space/state/` (the ENS commit secret and run outputs) out of git.

- `golden/` is read by the route crate's unit tests (`route/src/snapshot.rs`), so `cargo test` needs no
  files outside the repository. **`golden/signature.hex` is a test fixture, not a credential:** it was made
  by a throwaway test key (`0x042abe4EE65cBC71Bd11297DCea2d69e1650a596`, which holds no funds of value)
  over the synthetic golden Vote above (space `bloom-ballot-test.eth`, our test-space naming; proposal
  `keccak("bloom-ballot golden vector")`). It was never submitted to any sequencer.
- `responses/` holds recorded hub responses. Besides the step 0 recordings cited in `FINDINGS.md`, the
  `ballot_*` files were recorded on 2026-11-07 with the exact GraphQL queries the read routes send
  (`route/src/hub.rs`), and the route tests use them as fixtures.
