use cw_multi_test::{App, ContractWrapper};

/// Stores the base CW20 contract to the app.
pub fn store_cw20_token_code(app: &mut App) -> u64 {
    let contract = Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ));

    app.store_code(contract)
}

pub fn store_factory_code(app: &mut App) -> u64 {
    let contract = Box::new(
        ContractWrapper::new(
            incentive_factory::contract::execute,
            incentive_factory::contract::instantiate,
            incentive_factory::contract::query,
        )
        .with_migrate(incentive_factory::contract::migrate)
        .with_reply(incentive_factory::contract::reply),
    );

    app.store_code(contract)
}

/// Stores the fee collector contract to the app
pub fn store_incentive(app: &mut App) -> u64 {
    let contract = Box::new(
        ContractWrapper::new(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_migrate(crate::contract::migrate),
    );

    app.store_code(contract)
}
