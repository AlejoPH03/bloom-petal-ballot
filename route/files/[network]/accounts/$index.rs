// Bloom lists the wallets itself from its own projection; this route only
// refuses an unknown network.
petal::route_file!(
    spec: petal::static_dir_spec(),
    ctx_list: |ctx: &petal::Ctx| -> Result<Vec<petal::RouteChild>, petal::DispatchResponse> {
        let value = petal::param(ctx, "network")?;
        crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))?;
        Ok(Vec::new())
    },
);
