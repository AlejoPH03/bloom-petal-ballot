#!/usr/bin/env bash
# Gasless. Create ONE single-choice proposal in our space from the throwaway admin.
# usage: 08_new_proposal.sh "<title>" "<body>" <duration_minutes> [choice ...]
#   choices default to For Against Abstain; start = now, end = now + duration.
# Idempotent per title: refuses if a proposal with exactly that title already exists in the space.
# DRYRUN=1 builds + signs + verifies the envelope without POSTing.
# Keeps no signature on disk and prints no receipt (lib.sh scrub).
source "$(dirname "$0")/lib.sh"

[ $# -ge 3 ] || { echo "usage: $0 \"<title>\" \"<body>\" <duration_minutes> [choice ...]" >&2; exit 2; }
TITLE="$1"; BODY="$2"; MINUTES="$3"; shift 3
[[ "$MINUTES" =~ ^[1-9][0-9]*$ ]] || { echo "duration_minutes must be a positive integer" >&2; exit 2; }
if [ $# -gt 0 ]; then CHOICES=("$@"); else CHOICES=(For Against Abstain); fi
CHOICES_JSON=$(printf '%s\n' "${CHOICES[@]}" | jq -R . | jq -sc .)

S=$(space_id)

# Idempotency: exact title match among all proposals of the space (hub, any state)
EXISTING=$(gql "{ proposals(first: 1000, where: {space: \"$S\"}) { id title state } }" \
  | jq -r --arg t "$TITLE" '.data.proposals[] | select(.title == $t) | "\(.id) (\(.state))"')
if [ -n "$EXISTING" ]; then
  echo "refusing: proposal titled \"$TITLE\" already exists in $S: $EXISTING" >&2; exit 3
fi

SLUG=$(printf %s "$TITLE" | tr 'A-Z' 'a-z' | tr -cs 'a-z0-9' '-' | sed 's/^-//;s/-$//' | cut -c1-48)
OUT="$STATE/proposals/$SLUG"; mkdir -p "$STATE/proposals"

NOW=$(date +%s); BLOCK=$(( $(cast block-number) - 5 ))
# snapshot.js proposalTypes (src/sign/types.ts:140-158); hash e037b373... -> "proposal"
echo '{"Proposal":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"type","type":"string"},{"name":"title","type":"string"},{"name":"body","type":"string"},{"name":"discussion","type":"string"},{"name":"choices","type":"string[]"},{"name":"labels","type":"string[]"},{"name":"start","type":"uint64"},{"name":"end","type":"uint64"},{"name":"snapshot","type":"uint64"},{"name":"plugins","type":"string"},{"name":"privacy","type":"string"},{"name":"app","type":"string"}]}' > "$OUT.types.json"
jq -nc --arg f "$ADDR" --arg s "$S" --argjson ts $NOW --argjson b $BLOCK --argjson end $((NOW + MINUTES*60)) \
   --arg title "$TITLE" --arg body "$BODY" --argjson choices "$CHOICES_JSON" '{
  from:$f, space:$s, timestamp:$ts, type:"single-choice", title:$title, body:$body, discussion:"",
  choices:$choices, labels:[], start:$ts, end:$end, snapshot:$b, plugins:"{}", privacy:"", app:"bloom-ballot"}' > "$OUT.msg.json"

trap 'scrub "$OUT"' EXIT
sign_and_send "$OUT.types.json" "$OUT.msg.json" Proposal "$OUT" >/dev/null
if [ "${DRYRUN:-0}" = 1 ]; then
  jq -c '.data.message | {space,title,type,choices,start,end,snapshot}' "$OUT.envelope.json"
  rm -f "$OUT".*; exit 0
fi

P=$(jq -r '.id // empty' "$OUT.response.json")
[ -n "$P" ] || { echo "sequencer rejected the proposal:" >&2; jq -c 'del(.relayer.receipt)' "$OUT.response.json" >&2; exit 1; }
echo "proposal id: $P"
for i in $(seq 1 20); do
  R=$(gql "{ proposal(id: \"$P\") { id title choices type start end snapshot state } }")
  echo "$R" | jq -e '.data.proposal' >/dev/null && { echo "$R" | tee "$OUT.hub.json" | jq -c .data.proposal; exit 0; }
  sleep 3
done
echo "proposal not visible on hub yet" >&2; exit 1
