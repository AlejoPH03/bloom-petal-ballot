petal::route_file!(
    spec: petal::static_dir_spec(),
    list: vec![
        petal::file("status.json"),
        petal::dir("testnet"),
        petal::dir("mainnet"),
    ],
);
