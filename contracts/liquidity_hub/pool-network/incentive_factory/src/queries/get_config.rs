use cosmwasm_std::{Deps, StdError};
use pool_network::incentive_factory::GetConfigResponse;

use crate::state::CONFIG;

/// Retrieves the configuration of the contract.
pub fn get_config(deps: Deps) -> Result<GetConfigResponse, StdError> {
    CONFIG.load(deps.storage)
}
