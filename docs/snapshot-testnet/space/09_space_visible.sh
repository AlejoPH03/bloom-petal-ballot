#!/usr/bin/env bash
# Gasless. Re-signs the space settings of 05_create_space.sh with private:false
# (visible in listings), everything else unchanged. DRYRUN=1 signs but does not
# POST. Keeps no signature on disk (lib.sh scrub) and prints no receipt.
source "$(dirname "$0")/lib.sh"
S=$(space_id)
[ "$S" = "bloom-ballot-182c.eth" ] || { echo "refusing: only our own testnet space" >&2; exit 1; }
SETTINGS=$(jq -nc --arg a "$ADDR" '{
  name:"bloom-ballot test", about:"Throwaway sandbox for Bloom ballot petal integration tests. Not a real DAO.",
  network:"11155111", symbol:"VOTE", private:false,
  strategies:[{name:"ticket", network:"11155111", params:{symbol:"VOTE", value:1}}],
  admins:[$a], members:[], moderators:[],
  filters:{minScore:0, onlyMembers:true},
  validation:{name:"any", params:{}},
  voteValidation:{name:"any", params:{}},
  voting:{type:"single-choice", privacy:"", hideAbstain:false},
  categories:[], plugins:{}, treasuries:[], labels:[]
}')
OUT="$STATE/space-visible"
trap 'scrub "$OUT"' EXIT
echo '{"Space":[{"name":"from","type":"address"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"settings","type":"string"}]}' > "$OUT.types.json"
jq -nc --arg f "$ADDR" --arg s "$S" --argjson ts "$(date +%s)" --arg set "$SETTINGS" '{from:$f,space:$s,timestamp:$ts,settings:$set}' > "$OUT.msg.json"
sign_and_send "$OUT.types.json" "$OUT.msg.json" Space "$OUT" >/dev/null
scrub "$OUT"
if [ "${DRYRUN:-0}" = 1 ]; then
  echo "DRYRUN: signed, not POSTed. Settings that would be sent:"
  jq -c '.settings|fromjson' "$OUT.msg.json"
  exit 0
fi
jq -e .id "$OUT.response.json" >/dev/null || { echo "sequencer refused:" >&2; cat "$OUT.response.json" >&2; exit 1; }
jq -c '{id,ipfs,relayer:.relayer.address}' "$OUT.response.json"
for _ in $(seq 1 20); do
  R=$(gql "{ space(id: \"$S\") { id private } spaces(first: 1, where: {id_in: [\"$S\"]}) { id private } }")
  [ "$(echo "$R" | jq '.data.space.private')" = false ] && { echo "$R" | jq -c .data; exit 0; }
  sleep 3
done
echo "hub still shows the space private" >&2; exit 1
