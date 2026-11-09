// Open proposals where a delegate in the mandate's `follow` voted and this
// account has not, each with a ready votes/new.json body. Without follow
// entries it explains how to set them. Never writes the store and never
// votes. route/src/following.rs.
//
// Caps = compiled imports: bloom:http (hub, at most 3 calls per entry),
// bloom:store (the mandate), bloom:vfs.read (the account's address.evm).
petal::route_file!(
    spec: petal::account_read_spec().caps(&["bloom:http", "bloom:store", "bloom:vfs.read"]),
    read: |ctx: &petal::Ctx| {
        let network = match petal::param(ctx, "network").and_then(|value| {
            crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))
        }) {
            Ok(network) => network,
            Err(response) => return response,
        };
        let account = match petal::wallet_account(ctx) {
            Ok(account) => account,
            Err(response) => return response,
        };
        let scope = crate::votes::store::Scope::new(network, account.wallet(), account.number());
        let mut host = crate::votes::host::SdkHost::new(ctx, &account);
        match crate::following::following_document(&mut host, &scope) {
            Ok(document) => petal::read_json_value(&document),
            Err(error) => petal::error(-4, format!("following: {error}")),
        }
    },
);
