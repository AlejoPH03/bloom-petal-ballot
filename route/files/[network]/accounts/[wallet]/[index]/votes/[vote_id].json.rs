// One vote: status, readable summary (space, proposal title, choice label,
// reason), the frozen typed data, digest and verify commands, approval
// pointers, the sequencer receipt and a live `counted` check on the hub. The
// read never writes the store (docs/plan-votes.md §6). An unknown or invalid
// vote id is "not found".
//
// Caps = compiled imports: bloom:store (binding, record) and bloom:http (the
// live hub check). The voter address comes from the binding, so no vfs.read;
// account_read_spec's vfs.read is removed.
petal::route_file!(
    spec: petal::account_read_spec().caps(&["bloom:http", "bloom:store"]),
    read: |ctx: &petal::Ctx| {
        let network = match petal::param(ctx, "network").and_then(|value| {
            crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))
        }) {
            Ok(network) => network,
            Err(response) => return response,
        };
        let vote_id = match petal::param(ctx, "vote_id") {
            Ok(value) => match crate::votes::body::validate_route_vote_id(value) {
                Ok(vote_id) => vote_id,
                Err(_) => return petal::error(-1, "vote not found"),
            },
            Err(response) => return response,
        };
        let account = match petal::wallet_account(ctx) {
            Ok(account) => account,
            Err(response) => return response,
        };
        let scope = crate::votes::store::Scope::new(network, account.wallet(), account.number());
        let mut host = crate::votes::host::SdkHost::new(ctx, &account);
        match crate::votes::flow::vote_document(&mut host, &scope, vote_id) {
            Ok(Some(document)) => petal::read_json_value(&document),
            Ok(None) => petal::error(-1, "vote not found"),
            Err(error) => petal::error(-4, format!("read vote: {error}")),
        }
    },
);
