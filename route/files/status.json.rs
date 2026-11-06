petal::route_file!(
    spec: petal::static_read_spec(),
    read: |_ctx: &petal::Ctx| {
        let testnet = crate::Network::Testnet;
        let mainnet = crate::Network::Mainnet;
        petal::read_json_value(&serde_json::json!({
            "petal": "ballot",
            "version": "0.1.0",
            "sign_intent": "snapshot.vote",
            "networks": {
                // TODO(step0): verify host (crate::TESTNET_HUB)
                "testnet": { "hub": testnet.hub_url(), "sequencer": testnet.sequencer_url() },
                "mainnet": { "hub": mainnet.hub_url(), "sequencer": mainnet.sequencer_url() }
            },
            "docs": "AGENTS.md"
        }))
    },
);
