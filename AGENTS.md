# ballot operating contract

`ballot` will read Snapshot governance proposals and cast owner-approved,
gasless EIP-712 votes through Bloom signing (`bloom:sign`, intent
`snapshot.vote`). **Work in progress:** no route contacts an upstream or
submits anything. The only signing route is the temporary Step 1
`hello-sign.json`.

## Routes

| Route | Kind | Parameters | Read | Write | Capabilities |
| --- | --- | --- | --- | --- | --- |
| `/petals/ballot/` | dir | — | lists `status.json`, `testnet/`, `mainnet/` | — | none |
| `/petals/ballot/status.json` | file | — | static JSON: petal, version, sign intent, hub and sequencer URLs per network | — | none |
| `/petals/ballot/<network>/` | dir | `network` = `testnet` \| `mainnet` | lists `accounts/`; any other name is `not found` | — | none |
| `/petals/ballot/<network>/accounts/` | dir | `network` | wallets (listed by Bloom) | — | none |
| `/petals/ballot/<network>/accounts/<wallet>/` | dir | `wallet` (Bloom wallet id) | numbered accounts (listed by Bloom) | — | none |
| `/petals/ballot/<network>/accounts/<wallet>/<n>/` | dir | `wallet`, `index` | lists `hello-sign.json` | — | none |
| `/petals/ballot/<network>/accounts/<wallet>/<n>/hello-sign.json` | writable, **TEMPORARY (Step 1)** | `network`, `wallet`, `index` | last attempt: status, attempt, full `typed_data`, `preimage`, `claimed_hash`, pending ids and where the owner finds them, `signature_recovers_to_account`, `signature_sha256`, timestamps, and `verify` commands | body empty or `{"attempt": n}`: requests one exact `snapshot.vote` signature over a fixed test vote; nothing is submitted | `bloom:sign`, `bloom:store`, `bloom:vfs.read` |

`hello-sign.json` is a pipeline proof (docs/plan-hello-signature.md). Use
`bloom vfs write <path> --data '{"attempt":0}'` and retry with the **same**
body after each owner approval. A pending write fails with
`approval pending (<id>)`; read the file to see where the owner finds the
ceremony. Before the owner approves, run the read's `verify` commands: they
recompute the digest with Bloom's own `/tools/eip712/hash`, and the result
must equal the exact hash on the ceremony page. Signature bytes are never
stored or returned; only their sha256 and whether they recover to the
account's `address.evm`. See docs/known-limitations.md.

## Development skill

Petals are authored against the `bloom-petal-development` skill at
`.agents/skills/bloom-petal-development/SKILL.md` (written by `petal new`).
Load it before extending this Petal. It is part of the Petal package, so
editing it changes the package hash; Bloom does not serve it under
`/petals/ballot/`.

Route files own their parameter parsing and behaviour. `route/src` holds typed
helpers only (`Network`, input validators); it never dispatches on route
identity. Run `bash scripts/check-route-architecture.sh` and `petal check`
before pushing.
