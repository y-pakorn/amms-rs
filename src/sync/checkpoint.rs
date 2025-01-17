use std::{
    fs::read_to_string,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ethers::{providers::Middleware, types::H160};
use indicatif::ProgressBar;
use serde::{Deserialize, Serialize};
use tokio::task::{JoinHandle, JoinSet};

use crate::{
    amm::{
        factory::{AutomatedMarketMakerFactory, Factory},
        uniswap_v2::factory::UniswapV2Factory,
        uniswap_v3::factory::UniswapV3Factory,
        AMM,
    },
    constants::{MULTIPROGRESS, SPINNER_STYLE},
    errors::{AMMError, CheckpointError},
    sync,
};

use super::{amms_are_congruent, populate_amms};

#[derive(Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub timestamp: usize,
    pub block_number: u64,
    pub factories: Vec<Factory>,
    pub amms: Vec<AMM>,
}

impl Checkpoint {
    pub fn new(
        timestamp: usize,
        block_number: u64,
        factories: Vec<Factory>,
        amms: Vec<AMM>,
    ) -> Checkpoint {
        Checkpoint {
            timestamp,
            block_number,
            factories,
            amms,
        }
    }
}

//Get all pairs from last synced block and sync reserve values for each Dex in the `dexes` vec.
pub async fn sync_amms_from_checkpoint<M: 'static + Middleware>(
    path_to_checkpoint: &str,
    step: u64,
    middleware: Arc<M>,
) -> Result<(Vec<Factory>, Vec<AMM>), AMMError<M>> {
    let spinner = MULTIPROGRESS.add(
        ProgressBar::new_spinner()
            .with_style(SPINNER_STYLE.clone())
            .with_message("Syncing AMMs from checkpoint file..."),
    );
    spinner.enable_steady_tick(Duration::from_millis(200));

    let current_block = middleware
        .get_block_number()
        .await
        .map_err(AMMError::MiddlewareError)?
        .as_u64();

    let checkpoint: Checkpoint =
        serde_json::from_str(read_to_string(path_to_checkpoint)?.as_str())?;

    //Sort all of the pools from the checkpoint into uniswap_v2_pools and uniswap_v3_pools pools so we can sync them concurrently
    let (uniswap_v2_pools, uniswap_v3_pools, erc_4626_pools) = sort_amms(checkpoint.amms);

    let mut aggregated_amms = vec![];
    let mut handles = JoinSet::new();

    //Sync all uniswap v2 pools from checkpoint
    if !uniswap_v2_pools.is_empty() {
        batch_sync_amms_from_checkpoint(
            &mut handles,
            uniswap_v2_pools,
            current_block,
            middleware.clone(),
        )
        .await?;
    }

    //Sync all uniswap v3 pools from checkpoint
    if !uniswap_v3_pools.is_empty() {
        batch_sync_amms_from_checkpoint(
            &mut handles,
            uniswap_v3_pools,
            current_block,
            middleware.clone(),
        )
        .await?;
    }

    if !erc_4626_pools.is_empty() {
        // TODO: Batch sync erc4626 pools from checkpoint
        todo!(
            r#"""This function will produce an incorrect state if ERC4626 pools are present in the checkpoint. 
            This logic needs to be implemented into batch_sync_amms_from_checkpoint"""#
        );
    }

    //Sync all pools from the since synced block
    get_new_amms_from_range(
        &mut handles,
        checkpoint.factories.clone(),
        checkpoint.block_number,
        current_block,
        step,
        middleware.clone(),
    )
    .await?;

    while let Some(amms) = handles.join_next().await {
        aggregated_amms.extend(amms??);
    }

    //update the sync checkpoint
    construct_checkpoint(
        checkpoint.factories.clone(),
        &aggregated_amms,
        current_block,
        path_to_checkpoint,
    )?;

    spinner.finish_and_clear();

    Ok((checkpoint.factories, aggregated_amms))
}

pub async fn get_new_amms_from_range<M: 'static + Middleware>(
    handles: &mut JoinSet<Result<Vec<AMM>, AMMError<M>>>,
    factories: Vec<Factory>,
    from_block: u64,
    to_block: u64,
    step: u64,
    middleware: Arc<M>,
) -> Result<(), AMMError<M>> {
    //Create the filter with all the pair created events
    //Aggregate the populated pools from each thread
    for factory in factories.into_iter() {
        let middleware = middleware.clone();
        let spinner = MULTIPROGRESS.add(
            ProgressBar::new_spinner()
                .with_style(SPINNER_STYLE.clone())
                .with_message(format!("Fetching new pools from {}...", factory.address())),
        );
        spinner.enable_steady_tick(Duration::from_millis(200));

        //Spawn a new thread to get all pools and sync data for each dex
        handles.spawn(async move {
            let mut amms = factory
                .get_all_pools_from_logs(from_block, to_block, step, middleware.clone())
                .await?;

            factory
                .populate_amm_data(&mut amms, Some(to_block), middleware.clone())
                .await?;

            //Clean empty pools
            amms = sync::remove_empty_amms(amms);

            spinner.finish_and_clear();
            Ok::<_, AMMError<M>>(amms)
        });
    }

    Ok(())
}

pub async fn batch_sync_amms_from_checkpoint<M: 'static + Middleware>(
    handles: &mut JoinSet<Result<Vec<AMM>, AMMError<M>>>,
    amms: Vec<AMM>,
    block_number: u64,
    middleware: Arc<M>,
) -> Result<(), AMMError<M>> {
    let factory = match amms[0] {
        AMM::UniswapV2Pool(_) => Some(Factory::UniswapV2Factory(UniswapV2Factory::new(
            H160::zero(),
            0,
            0,
        ))),

        AMM::UniswapV3Pool(_) => Some(Factory::UniswapV3Factory(UniswapV3Factory::new(
            H160::zero(),
            0,
        ))),

        AMM::ERC4626Vault(_) => None,
    };

    //Spawn a new thread to get all pools and sync data for each dex
    if let Some(_factory) = factory {
        if amms_are_congruent(&amms) {
            for amms in amms.chunks(50_000) {
                let mut amms = amms.to_vec();
                let middleware = middleware.clone();
                handles.spawn(async move {
                    //Get all pool data via batched calls
                    amms = populate_amms(&amms, block_number, None, middleware).await?;
                    //factory
                    //.populate_amm_data(&mut amms, block_number, middleware)
                    //.await?;
                    //Clean empty pools
                    amms = sync::remove_empty_amms(amms);
                    Ok::<_, AMMError<M>>(amms)
                });
            }
            Ok(())
        } else {
            Err(AMMError::IncongruentAMMs)
        }
    } else {
        Ok(())
    }
}

pub fn sort_amms(amms: Vec<AMM>) -> (Vec<AMM>, Vec<AMM>, Vec<AMM>) {
    let mut uniswap_v2_pools = vec![];
    let mut uniswap_v3_pools = vec![];
    let mut erc_4626_vaults = vec![];
    for amm in amms {
        match amm {
            AMM::UniswapV2Pool(_) => uniswap_v2_pools.push(amm),
            AMM::UniswapV3Pool(_) => uniswap_v3_pools.push(amm),
            AMM::ERC4626Vault(_) => erc_4626_vaults.push(amm),
        }
    }

    (uniswap_v2_pools, uniswap_v3_pools, erc_4626_vaults)
}

pub async fn get_new_pools_from_range<M: 'static + Middleware>(
    factories: Vec<Factory>,
    from_block: u64,
    to_block: u64,
    step: u64,
    middleware: Arc<M>,
) -> Vec<JoinHandle<Result<Vec<AMM>, AMMError<M>>>> {
    //Create the filter with all the pair created events
    //Aggregate the populated pools from each thread
    let mut handles = vec![];

    for factory in factories {
        let middleware = middleware.clone();

        //Spawn a new thread to get all pools and sync data for each dex
        handles.push(tokio::spawn(async move {
            let mut pools = factory
                .get_all_pools_from_logs(from_block, to_block, step, middleware.clone())
                .await?;

            factory
                .populate_amm_data(&mut pools, Some(to_block), middleware.clone())
                .await?;

            //Clean empty pools
            pools = sync::remove_empty_amms(pools);

            Ok::<_, AMMError<M>>(pools)
        }));
    }

    handles
}

pub fn construct_checkpoint(
    factories: Vec<Factory>,
    amms: &[AMM],
    latest_block: u64,
    checkpoint_path: &str,
) -> Result<(), CheckpointError> {
    let checkpoint = Checkpoint::new(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64() as usize,
        latest_block,
        factories,
        amms.to_vec(),
    );

    std::fs::write(checkpoint_path, serde_json::to_string_pretty(&checkpoint)?)?;

    Ok(())
}

//Deconstructs the checkpoint into a Vec<AMM>
pub fn deconstruct_checkpoint(checkpoint_path: &str) -> Result<(Vec<AMM>, u64), CheckpointError> {
    let checkpoint: Checkpoint = serde_json::from_str(read_to_string(checkpoint_path)?.as_str())?;
    Ok((checkpoint.amms, checkpoint.block_number))
}
