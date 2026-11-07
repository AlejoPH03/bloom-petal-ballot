#!/usr/bin/env bash
# NEEDS SEPOLIA ETH (2 txs). Mints free MockUSDC and approves the ENSv2 ETHRegistrar.
source "$(dirname "$0")/lib.sh"
L=$(label)
read -r BASE PREM < <(cast call $ETH_REGISTRAR 'getRegisterPrice(string,uint64,address)(uint256,uint256)' "$L" $DURATION $MOCK_USDC | awk '{print $1}' | paste -sd' ')
AMT=$(( (BASE + PREM) * 2 ))   # 2x headroom
cast send --private-key "$(cat "$KEY_FILE")" $MOCK_USDC 'mint(address,uint256)' "$ADDR" $AMT --json | jq '{tx:.transactionHash,status}'
cast send --private-key "$(cat "$KEY_FILE")" $MOCK_USDC 'approve(address,uint256)' $ETH_REGISTRAR $AMT --json | jq '{tx:.transactionHash,status}'
echo "USDC balance: $(cast call $MOCK_USDC 'balanceOf(address)(uint256)' "$ADDR")  allowance: $(cast call $MOCK_USDC 'allowance(address,address)(uint256)' "$ADDR" $ETH_REGISTRAR)"
