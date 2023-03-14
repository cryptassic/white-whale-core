use cosmwasm_std::{
    to_binary, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, QueryRequest, ReplyOn, Response,
    StdError, SubMsg, Timestamp, Uint128, Uint64, WasmMsg, WasmQuery,
};

use terraswap::asset::{Asset, AssetInfo};
use white_whale::whale_lair::{BondingWeightResponse, QueryMsg};

use crate::contract::EPOCH_CREATION_REPLY_ID;
use crate::helpers::{validate_grace_period, DAY_IN_SECONDS};
use crate::msg::ExecuteMsg;
use crate::state::{
    get_claimable_epochs, get_current_epoch, get_expiring_epoch, Epoch, CONFIG, EPOCHS,
    LAST_CLAIMED_EPOCH,
};
use crate::ContractError::Std;
use crate::{helpers, ContractError};

/// Creates a new epoch, forwarding available tokens from epochs that are past the grace period.
pub fn create_new_epoch(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let current_epoch = get_current_epoch(deps.as_ref())?;

    if env
        .block
        .time
        .minus_seconds(current_epoch.start_time.seconds())
        .seconds()
        < config.epoch_config.duration.u64()
    {
        return Err(ContractError::CurrentEpochNotExpired {});
    }

    // move this stuff to reply
    // // forward fees from previous epoch to the new one
    // let expiring_epoch = get_expiring_epoch(deps.as_ref())?;
    //
    // let unclaimed_fees = expiring_epoch
    //     .map(|epoch| epoch.available)
    //     .unwrap_or_default();
    //
    // // todo this fees will come from sending them from the fee collector to this contract
    // // todo should think on how to "deposit" fees to the latest epoch, i.e. when sending the tokens from the fee collector here
    // //  maybe with a submessage?
    // fees = helpers::aggregate_fees(fees, unclaimed_fees);

    let start_time = if current_epoch.id == 0 && current_epoch.start_time == Timestamp::default() {
        // if it's the very first epoch, set the start time to the genesis epoch
        Timestamp::from_seconds(config.epoch_config.genesis_epoch.u64())
    } else {
        current_epoch
            .start_time
            .plus_seconds(config.epoch_config.duration.u64())
    };

    // let new_epoch = Epoch {
    //     id: current_epoch
    //         .id
    //         .checked_add(1)
    //         .ok_or_else(|| StdError::generic_err("couldn't compute epoch id"))?,
    //     start_time,
    //     total: fees.clone(),
    //     available: fees.clone(),
    //     claimed: vec![],
    // };

    let new_epoch = Epoch {
        id: current_epoch
            .id
            .checked_add(1)
            .ok_or_else(|| StdError::generic_err("couldn't compute epoch id"))?,
        start_time,
        total: vec![],
        available: vec![],
        claimed: vec![],
    };

    //EPOCHS.save(deps.storage, &new_epoch.id.to_be_bytes(), &new_epoch)?;

    Ok(Response::new()
        .add_submessage(SubMsg {
            id: EPOCH_CREATION_REPLY_ID,
            msg: CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.fee_collector.to_string(),
                msg: to_binary(&ExecuteMsg::ForwardFees {})?,
                funds: vec![],
            }),
            gas_limit: None,
            reply_on: ReplyOn::Success,
        })
        .set_data(to_binary(&new_epoch))
        .add_attributes(vec![
            ("action", "create_new_epoch".to_string()),
            ("new_epoch", new_epoch.id.to_string()),
            ("fees_to_distribute", format!("{:?}", fees)),
        ]))
}

pub fn claim(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    // Query the fee share of the sender based on the ratio of his weight and the global weight at the current moment

    let config = CONFIG.load(deps.storage)?;
    let staking_weight_response: BondingWeightResponse =
        deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.staking_contract_addr.to_string(),
            msg: to_binary(&QueryMsg::Weight {
                address: info.sender.to_string(),
            })?,
        }))?;

    let fee_share = staking_weight_response.share;

    let mut claimable_epochs = get_claimable_epochs(deps.as_ref())?;
    let last_claimed_epoch = LAST_CLAIMED_EPOCH.may_load(deps.storage, &info.sender)?;

    // filter out epochs that have already been claimed by the user
    if let Some(last_claimed_epoch) = last_claimed_epoch {
        claimable_epochs.retain(|epoch| epoch.id > last_claimed_epoch);

        // the user has already claimed fees on all claimable epochs
        if claimable_epochs.is_empty() {
            return Err(ContractError::NothingToClaim {});
        }
    };

    let mut claimable_fees = vec![];
    for mut epoch in claimable_epochs.clone() {
        for fee in epoch.total.iter() {
            let reward = fee.amount * fee_share;

            // make sure the reward is sound
            let _ = epoch
                .available
                .iter()
                .find(|available_fee| available_fee.info == fee.info)
                .map(|available_fee| {
                    if reward > available_fee.amount {
                        return Err(ContractError::InvalidReward {});
                    }
                    Ok(())
                })
                .ok_or_else(|| StdError::generic_err("Invalid fee"))?;

            // add the reward to the claimable fees
            claimable_fees = helpers::aggregate_fees(
                claimable_fees,
                vec![Asset {
                    info: fee.info.clone(),
                    amount: reward,
                }],
            );

            // modify the epoch to reflect the new available and claimed amount
            epoch.available.iter_mut().for_each(|available_fee| {
                if available_fee.info == fee.info {
                    available_fee.amount -= reward;
                }
            });

            epoch.claimed.iter_mut().for_each(|claimed_fee| {
                if claimed_fee.info == fee.info {
                    claimed_fee.amount += reward;
                }
            });

            EPOCHS.save(deps.storage, &epoch.id.to_be_bytes(), &epoch)?;
        }
    }

    // update the last claimed epoch for the user
    LAST_CLAIMED_EPOCH.save(deps.storage, &info.sender, &claimable_epochs[0].id)?;

    // send funds to the user
    let mut messages = vec![];
    for fee in claimable_fees {
        messages.push(fee.into_msg(info.sender.clone())?);
    }

    Ok(Response::new()
        .add_attributes(vec![("action", "claim")])
        .add_messages(messages))
}

/// Updates the [Config] of the contract
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    owner: Option<String>,
    staking_contract_addr: Option<String>,
    fee_collector_addr: Option<String>,
    grace_period: Option<Uint64>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }

    if let Some(staking_contract_addr) = staking_contract_addr {
        config.staking_contract_addr = deps.api.addr_validate(&staking_contract_addr)?;
    }

    if let Some(fee_collector_addr) = fee_collector_addr {
        config.fee_collector_addr = deps.api.addr_validate(&fee_collector_addr)?;
    }

    let mut messages = vec![];
    if let Some(grace_period) = grace_period {
        validate_grace_period(&grace_period)?;
        if grace_period < config.grace_period {
            let claimable_epochs = get_claimable_epochs(deps.as_ref())?;
            // check if grace period is lower than the current one. If so, we need to forward the fees
            // to a new epoch

            let (_, expired_epochs) = claimable_epochs.split_at(grace_period.u64() as usize);

            let mut forwarding_assets = vec![];
            for epoch in expired_epochs {
                forwarding_assets =
                    helpers::aggregate_fees(forwarding_assets, epoch.available.clone());
            }

            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_binary(&ExecuteMsg::NewEpoch {
                    fees: forwarding_assets.clone(),
                })?,
                funds: helpers::to_coins(forwarding_assets)?,
            }));
        }

        config.grace_period = grace_period;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        ("action", "update_config".to_string()),
        ("owner", config.owner.to_string()),
        (
            "staking_contract_addr",
            config.staking_contract_addr.to_string(),
        ),
        ("fee_collector_addr", config.fee_collector_addr.to_string()),
        ("grace_period", config.grace_period.to_string()),
    ]))
}
