// Spaces are reached by exact path (spaces/<space-id>/); the hub has too many
// to list, so this directory is intentionally empty. See AGENTS.md.
petal::route_file!(
    spec: petal::static_dir_spec(),
    ctx_list: |ctx: &petal::Ctx| -> Result<Vec<petal::RouteChild>, petal::DispatchResponse> {
        let value = petal::param(ctx, "network")?;
        crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))?;
        Ok(Vec::new())
    },
);
