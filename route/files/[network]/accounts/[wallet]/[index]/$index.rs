petal::route_file!(
    spec: petal::static_dir_spec(),
    ctx_list: |ctx: &petal::Ctx| -> Result<Vec<petal::RouteChild>, petal::DispatchResponse> {
        let value = petal::param(ctx, "network")?;
        crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))?;
        Ok(vec![petal::file("inbox.json"), petal::file("following.json"), petal::writable("mandate.json"), petal::dir("votes")])
    },
);
