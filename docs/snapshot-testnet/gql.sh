#!/usr/bin/env bash
# usage: gql.sh '<graphql query>'  -> POSTs to testnet hub
HUB=${HUB:-https://testnet.hub.snapshot.org/graphql}
jq -n --arg q "$1" '{query:$q}' | curl -s -m 30 -X POST "$HUB" -H 'content-type: application/json' -d @-
