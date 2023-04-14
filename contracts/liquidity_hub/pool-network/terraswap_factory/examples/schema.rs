use cosmwasm_schema::write_api;

use white_whale::pool_network::factory::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};

fn main() {
    write_api! {
        name: "terraswap-factory",
        version: "1.0.3",
        instantiate: InstantiateMsg,
        query: QueryMsg,
        execute: ExecuteMsg,
        migrate: MigrateMsg,
    }
}
