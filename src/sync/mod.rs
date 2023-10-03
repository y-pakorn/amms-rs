use crate::{
    amm::{
        factory::{AutomatedMarketMakerFactory, Factory},
        uniswap_v2, uniswap_v3, AutomatedMarketMaker, AMM,
    },
    constants::{MULTIPROGRESS, SPINNER_STYLE},
    errors::AMMError,
};

use ethers::providers::Middleware;
use indicatif::ProgressBar;
use std::{panic::resume_unwind, sync::Arc, time::Duration};
use tokio::task::JoinSet;

pub mod checkpoint;

pub async fn sync_amms<M: 'static + Middleware>(
    factories: Vec<Factory>,
    middleware: Arc<M>,
    checkpoint_path: Option<&str>,
    step: u64,
) -> Result<(Vec<AMM>, u64), AMMError<M>> {
    let spinner = MULTIPROGRESS.add(
        ProgressBar::new_spinner()
            .with_style(SPINNER_STYLE.clone())
            .with_message("Syncing AMMs..."),
    );
    spinner.enable_steady_tick(Duration::from_millis(200));

    let current_block = middleware
        .get_block_number()
        .await
        .map_err(AMMError::MiddlewareError)?
        .as_u64();

    //Aggregate the populated pools from each thread
    let mut aggregated_amms: Vec<AMM> = vec![];
    let mut handles = JoinSet::new();

    //For each dex supplied, get all pair created events and get reserve values
    for factory in factories.clone() {
        let middleware = middleware.clone();

        //Spawn a new thread to get all pools and sync data for each dex
        handles.spawn(async move {
            let factory_spinner = MULTIPROGRESS.add(
                ProgressBar::new_spinner()
                    .with_style(SPINNER_STYLE.clone())
                    .with_message(format!("Getting all pools from: {}", factory.address())),
            );
            factory_spinner.enable_steady_tick(Duration::from_millis(200));
            //Get all of the amms from the factory
            let mut amms: Vec<AMM> = factory
                .get_all_amms(Some(current_block), middleware.clone(), step)
                .await?;
            factory_spinner.set_message(format!(
                "Populating pool's data from: {}",
                factory.address()
            ));
            populate_amms(&mut amms, current_block, middleware.clone()).await?;

            //Clean empty pools
            amms = remove_empty_amms(amms);

            // If the factory is UniswapV2, set the fee for each pool according to the factory fee
            if let Factory::UniswapV2Factory(factory) = factory {
                for amm in amms.iter_mut() {
                    if let AMM::UniswapV2Pool(ref mut pool) = amm {
                        pool.fee = factory.fee;
                    }
                }
            }

            factory_spinner.finish_and_clear();

            Ok::<_, AMMError<M>>(amms)
        });
    }

    while let Some(amm) = handles.join_next().await {
        match amm {
            Ok(sync_result) => aggregated_amms.extend(sync_result?),
            Err(err) => {
                {
                    if err.is_panic() {
                        // Resume the panic on the main task
                        resume_unwind(err.into_panic());
                    }
                }
            }
        }
    }

    //Save a checkpoint if a path is provided

    if let Some(checkpoint_path) = checkpoint_path {
        spinner.set_message("Saving checkpoint...");
        checkpoint::construct_checkpoint(
            factories,
            &aggregated_amms,
            current_block,
            checkpoint_path,
        )?;
    }

    spinner.finish_and_clear();

    //Return the populated aggregated amms vec
    Ok((aggregated_amms, current_block))
}

pub fn amms_are_congruent(amms: &[AMM]) -> bool {
    let expected_amm = &amms[0];

    for amm in amms {
        if std::mem::discriminant(expected_amm) != std::mem::discriminant(amm) {
            return false;
        }
    }
    true
}

//Gets all pool data and sync reserves
pub async fn populate_amms<M: Middleware>(
    amms: &mut [AMM],
    block_number: u64,
    middleware: Arc<M>,
) -> Result<(), AMMError<M>> {
    if amms_are_congruent(amms) {
        match amms[0] {
            AMM::UniswapV2Pool(_) => {
                let step = 127; //Max batch size for call
                for amm_chunk in amms.chunks_mut(step) {
                    uniswap_v2::batch_request::get_amm_data_batch_request(
                        amm_chunk,
                        middleware.clone(),
                    )
                    .await?;
                }
            }

            AMM::UniswapV3Pool(_) => {
                let step = 76; //Max batch size for call
                for amm_chunk in amms.chunks_mut(step) {
                    uniswap_v3::batch_request::get_amm_data_batch_request(
                        amm_chunk,
                        block_number,
                        middleware.clone(),
                    )
                    .await?;
                }
            }

            // TODO: Implement batch request
            AMM::ERC4626Vault(_) => {
                for amm in amms {
                    amm.populate_data(None, middleware.clone()).await?;
                }
            }
        }
    } else {
        return Err(AMMError::IncongruentAMMs);
    }

    //For each pair in the pairs vec, get the pool data
    Ok(())
}

pub fn remove_empty_amms(amms: Vec<AMM>) -> Vec<AMM> {
    let mut cleaned_amms = vec![];

    for amm in amms.into_iter() {
        match amm {
            AMM::UniswapV2Pool(ref uniswap_v2_pool) => {
                if !uniswap_v2_pool.token_a.is_zero() && !uniswap_v2_pool.token_b.is_zero() {
                    cleaned_amms.push(amm)
                }
            }
            AMM::UniswapV3Pool(ref uniswap_v3_pool) => {
                if !uniswap_v3_pool.token_a.is_zero() && !uniswap_v3_pool.token_b.is_zero() {
                    cleaned_amms.push(amm)
                }
            }
            AMM::ERC4626Vault(ref erc4626_vault) => {
                if !erc4626_vault.vault_token.is_zero() && !erc4626_vault.asset_token.is_zero() {
                    cleaned_amms.push(amm)
                }
            }
        }
    }

    cleaned_amms
}
