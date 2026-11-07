// One Snapshot space. Validates both segments locally; makes no request.
petal::route_file!(
    spec: petal::static_dir_spec(),
    ctx_list: |ctx: &petal::Ctx| -> Result<Vec<petal::RouteChild>, petal::DispatchResponse> {
        let network = petal::param(ctx, "network")?;
        crate::Network::parse(network).map_err(|error| petal::error(-1, error.message()))?;
        let space = petal::param(ctx, "space")?;
        crate::validate_space(space).map_err(|error| petal::error(-3, error.message()))?;
        Ok(vec![petal::file("proposals.json")])
    },
);
