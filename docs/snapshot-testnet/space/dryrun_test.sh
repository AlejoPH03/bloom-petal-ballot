#!/usr/bin/env bash
# Offline test: builds + signs the space, proposal and vote envelopes with DRYRUN=1.
# Nothing is POSTed; hub polling is stripped. Leaves state/ as it found it.
set -euo pipefail
export DRYRUN=1
cd "$(dirname "$0")"
# strip hub-polling tails so each script stops right after sign_and_send
run() { sed -e '/^for i in/,$d' -e '/^P=$(jq -r .id "$STATE\/proposal.response.json"); echo/d' -e '/^echo "vp:/d' -e '/^Q=/,$d' "$1" | timeout 60 bash -s; }

run 05_create_space.sh
jq -c '.data.message|{from,space,timestamp,settings:(.settings|fromjson|{strategies,filters,validation,voteValidation,voting})}' state/space.envelope.json
run 06_create_proposal.sh
jq -c '.data.message|{from,space,type,title,choices,start,end,snapshot,app}' state/proposal.envelope.json
echo '{"id":"0x3b1261975f64a6e11b3f365280580fbe0e7e7589799f122f4c0bcd61021bdcc6"}' > state/proposal.response.json  # synthetic id
run 07_vote.sh
jq -c '.data.message' state/vote.envelope.json
rm -f state/space.* state/proposal.* state/vote.*
echo "dry run OK"
