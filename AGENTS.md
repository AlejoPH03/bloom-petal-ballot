# ballot operating contract

`ballot` will read Snapshot governance proposals and cast owner-approved,
gasless EIP-712 votes through Bloom signing (`bloom:sign`, intent
`snapshot.vote`). **Work in progress:** no route contacts an upstream, writes,
or signs yet.

## Routes (milestone 0)

| Route | Kind | Parameters | Read | Write | Capabilities |
| --- | --- | --- | --- | --- | --- |
| `/petals/ballot/` | dir | — | lists `status.json`, `testnet/`, `mainnet/` | — | none |
| `/petals/ballot/status.json` | file | — | static JSON: petal, version, sign intent, hub and sequencer URLs per network | — | none |
| `/petals/ballot/<network>/` | dir | `network` = `testnet` \| `mainnet` | empty for now; any other name is `not found` | — | none |

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
