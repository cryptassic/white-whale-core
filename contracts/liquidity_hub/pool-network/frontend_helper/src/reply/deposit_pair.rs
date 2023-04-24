use cosmwasm_std::{to_binary, DepsMut, Env, Reply, Response, WasmMsg};
use white_whale::pool_network::{asset::AssetInfo, frontend_helper::TempState};

use crate::{
    error::ContractError,
    state::{CONFIG, TEMP_STATE},
};

/// The reply ID for submessages after depositing to the pair contract.
pub const DEPOSIT_PAIR_REPLY_ID: u64 = 1;

/// Triggered after a new deposit is made to a pair.
///
/// Triggered to allow us to register the new contract in state.
pub fn deposit_pair(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    msg.result
        .into_result()
        .map_err(|e| ContractError::DepositCallback { reason: e })?;

    let TempState {
        unbonding_duration,
        receiver,
        pair_addr,
    } = TEMP_STATE.load(deps.storage)?;
    let receiver = deps.api.addr_humanize(&receiver)?;
    let pair_address = deps.api.addr_humanize(&pair_addr)?;

    // now perform the incentive position creation
    let config = CONFIG.load(deps.storage)?;
    let incentive_factory_address = deps.api.addr_humanize(&config.incentive_factory_addr)?;

    let pair_info: white_whale::pool_network::asset::PairInfo = deps.querier.query_wasm_smart(
        pair_address.clone(),
        &white_whale::pool_network::pair::QueryMsg::Pair {},
    )?;

    let incentive_address: white_whale::pool_network::incentive_factory::GetIncentiveResponse =
        deps.querier.query_wasm_smart(
            incentive_factory_address,
            &white_whale::pool_network::incentive_factory::QueryMsg::Incentive {
                lp_address: pair_info.liquidity_token.clone(),
            },
        )?;
    // return an error if there was no incentive address
    let incentive_address = incentive_address.map_or_else(
        || {
            Err(ContractError::MissingIncentive {
                pair_address: pair_address.to_string(),
            })
        },
        Ok,
    )?;

    // compute current LP token amount
    let mut messages = vec![];
    let mut funds = vec![];
    let lp_amount = match pair_info.liquidity_token {
        AssetInfo::NativeToken { denom } => {
            // ask the bank module
            let balance = deps.querier.query_balance(env.contract.address, denom)?;

            // add the funds to the message
            funds.push(balance.clone());

            balance.amount
        }
        AssetInfo::Token { contract_addr } => {
            let balance: cw20::BalanceResponse = deps.querier.query_wasm_smart(
                contract_addr.clone(),
                &cw20::Cw20QueryMsg::Balance {
                    address: env.contract.address.into_string(),
                },
            )?;

            // add a message to increase allowance on the incentive contract
            // to spend our new LP tokens
            messages.push(WasmMsg::Execute {
                contract_addr,
                msg: to_binary(&cw20::Cw20ExecuteMsg::IncreaseAllowance {
                    spender: incentive_address.to_string(),
                    amount: balance.balance,
                    expires: None,
                })?,
                funds: vec![],
            });

            balance.balance
        }
    };

    Ok(Response::new()
        .add_messages(messages)
        .add_message(WasmMsg::Execute {
            contract_addr: incentive_address.into_string(),
            msg: to_binary(
                &white_whale::pool_network::incentive::ExecuteMsg::OpenPosition {
                    amount: lp_amount,
                    unbonding_duration,
                    receiver: Some(receiver.into_string()),
                },
            )?,
            funds,
        }))
}
