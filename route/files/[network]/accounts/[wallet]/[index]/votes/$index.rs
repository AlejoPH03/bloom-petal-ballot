// The account's votes directory: new.json plus one <vote_id>.json per vote
// record or binding in this account's store. Imports bloom:store only
// (store_list); store_dir_spec's vfs.read is not imported, so it is removed
// (docs/plan-votes.md §7, SKILL.md "Declare least capabilities").
petal::route_file!(
    spec: petal::store_dir_spec().caps(&["bloom:store"]),
    ctx_list: |ctx: &petal::Ctx| -> Result<Vec<petal::RouteChild>, petal::DispatchResponse> {
        let value = petal::param(ctx, "network")?;
        let network = crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))?;
        let account = petal::wallet_account(ctx)?;
        let scope = crate::votes::store::Scope::new(network, account.wallet(), account.number());
        let mut host = crate::votes::host::SdkHost::new(ctx, &account);
        let ids = crate::votes::flow::list_vote_ids(&mut host, &scope)
            .map_err(|error| petal::error(-4, format!("list votes: {error}")))?;
        let mut children = vec![petal::writable("new.json")];
        children.extend(ids.into_iter().map(|id| petal::file(format!("{id}.json"))));
        Ok(children)
    },
);
