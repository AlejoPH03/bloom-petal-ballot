// The agent's starting point: open proposals in the mandate's spaces (first
// 10), whether this account voted, closing_soon and result alerts. Without
// mandate spaces it explains how to set them. Never writes the store.
// docs/plan-mandate-inbox.md.
//
// Caps = compiled imports: bloom:http (hub, at most 2 calls per space),
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
        match crate::inbox::inbox_document(&mut host, &scope) {
            Ok(document) => petal::read_json_value(&document),
            Err(error) => petal::error(-4, format!("inbox: {error}")),
        }
    },
);
