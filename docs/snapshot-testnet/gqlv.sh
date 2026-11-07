#!/usr/bin/env bash
# usage: gqlv.sh <query.graphql> '<variables JSON>'  -> POSTs {query, variables} to the testnet hub
# (same request shape as route/src/hub.rs HubQuery::body; values travel only as variables)
HUB=${HUB:-https://testnet.hub.snapshot.org/graphql}
jq -n --rawfile q "$1" --argjson v "$2" '{query:($q|rtrimstr("\n")), variables:$v}' \
  | curl -s -m 30 -X POST "$HUB" -H 'content-type: application/json' -H 'accept: application/json' -d @-
