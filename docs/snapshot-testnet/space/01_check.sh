#!/usr/bin/env bash
# No funds needed. Shows address, balance, chosen label, availability and price.
source "$(dirname "$0")/lib.sh"
L=$(label)
echo "address:   $ADDR"
echo "balance:   $(cast balance --ether "$ADDR") SepoliaETH"
echo "label:     $L  (space id: $(space_id))"
echo "available: $(cast call $ETH_REGISTRAR 'isAvailable(string)(bool)' "$L")"
echo "price:     $(cast call $ETH_REGISTRAR 'getRegisterPrice(string,uint64,address)(uint256,uint256)' "$L" $DURATION $MOCK_USDC | tr '\n' ' ')(base premium, 6-dec MockUSDC)"
echo "gas price: $(cast gas-price) wei"
