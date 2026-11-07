// A space's active proposals plus its 10 most recent, deduplicated, newest
// first. One POST to the network's fixed hub GraphQL endpoint; the space id
// travels only in GraphQL variables. Imports bloom:http only.
petal::route_file!(
    spec: petal::http_read_spec(30_000),
    read: |ctx: &petal::Ctx| {
        let network = match petal::param(ctx, "network").and_then(|value| {
            crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))
        }) {
            Ok(network) => network,
            Err(response) => return response,
        };
        let space = match petal::param(ctx, "space") {
            Ok(space) => space,
            Err(response) => return response,
        };
        let query = match crate::proposals::proposals_query(space) {
            Ok(query) => query,
            Err(error) => return petal::error(-3, error.message()),
        };
        let data: crate::proposals::ProposalsData = match crate::hub::fetch(network, &query) {
            Ok(data) => data,
            Err(error) => return error.response(network),
        };
        petal::read_json_value(&crate::proposals::proposals_document(
            network,
            space,
            data,
            crate::hub::now_secs(),
        ))
    },
);
