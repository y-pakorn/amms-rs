use std::sync::Arc;

use async_trait::async_trait;
use ethers::{
    abi::RawLog,
    prelude::EthEvent,
    providers::Middleware,
    types::{Log, H160, H256, U256},
};

use indicatif::ProgressBar;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::{
    amm::{
        factory::{AutomatedMarketMakerFactory, TASK_LIMIT},
        AMM,
    },
    constants::{MULTIPROGRESS, SYNC_BAR_STYLE},
    errors::AMMError,
};

use super::{batch_request, UniswapV2Pool};

use ethers::prelude::abigen;

abigen!(
    IUniswapV2Factory,
    r#"[
        function getPair(address tokenA, address tokenB) external view returns (address pair)
        function allPairs(uint256 index) external view returns (address)
        event PairCreated(address indexed token0, address indexed token1, address pair, uint256)
        function allPairsLength() external view returns (uint256)

    ]"#;
);

pub const PAIR_CREATED_EVENT_SIGNATURE: H256 = H256([
    13, 54, 72, 189, 15, 107, 168, 1, 52, 163, 59, 169, 39, 90, 197, 133, 217, 211, 21, 240, 173,
    131, 85, 205, 222, 253, 227, 26, 250, 40, 208, 233,
]);

#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UniswapV2Factory {
    pub address: H160,
    pub creation_block: u64,
    pub fee: u32,
}

impl UniswapV2Factory {
    pub fn new(address: H160, creation_block: u64, fee: u32) -> UniswapV2Factory {
        UniswapV2Factory {
            address,
            creation_block,
            fee,
        }
    }

    pub async fn get_all_pairs_via_batched_calls<M: 'static + Middleware>(
        self,
        middleware: Arc<M>,
    ) -> Result<Vec<AMM>, AMMError<M>> {
        let factory = IUniswapV2Factory::new(self.address, middleware.clone());

        let pairs_length: U256 = factory.all_pairs_length().call().await?;
        let progress = MULTIPROGRESS.add(
            ProgressBar::new(pairs_length.as_u64())
                .with_style(SYNC_BAR_STYLE.clone())
                .with_message(format!("Getting all pools from: {}", self.address)),
        );

        let step = 766; //max batch size for this call until codesize is too large
        let mut idx_from = U256::zero();
        let mut idx_to = if step > pairs_length.as_usize() {
            pairs_length
        } else {
            U256::from(step)
        };
        let mut handles = JoinSet::new();
        let mut amms = vec![];

        for _ in (0..pairs_length.as_u128()).step_by(step) {
            let middleware = middleware.clone();
            let progress = progress.clone();
            handles.spawn(async move {
                let pairs = batch_request::get_pairs_batch_request(
                    self.address,
                    idx_from,
                    idx_to,
                    middleware,
                )
                .await?;
                progress.inc(idx_to.as_u64() - idx_from.as_u64());
                Ok::<_, AMMError<M>>(pairs)
            });

            idx_from = idx_to;

            if idx_to + step > pairs_length {
                idx_to = pairs_length - 1
            } else {
                idx_to = idx_to + step;
            }

            if handles.len() == TASK_LIMIT {
                Self::process_amm_from_requests(&mut amms, handles).await?;
                handles = JoinSet::new();
            }
        }

        Self::process_amm_from_requests(&mut amms, handles).await?;

        progress.finish_and_clear();

        Ok(amms)
    }

    pub async fn process_amm_from_requests<M: 'static + Middleware>(
        amms: &mut Vec<AMM>,
        mut set: JoinSet<Result<Vec<H160>, AMMError<M>>>,
    ) -> Result<(), AMMError<M>> {
        while let Some(pair) = set.join_next().await {
            for address in pair?? {
                let amm = UniswapV2Pool {
                    address,
                    ..Default::default()
                };

                amms.push(AMM::UniswapV2Pool(amm));
            }
        }
        Ok(())
    }
}

#[async_trait]
impl AutomatedMarketMakerFactory for UniswapV2Factory {
    fn address(&self) -> H160 {
        self.address
    }

    fn amm_created_event_signature(&self) -> H256 {
        PAIR_CREATED_EVENT_SIGNATURE
    }

    async fn new_amm_from_log<M: 'static + Middleware>(
        &self,
        log: Log,
        middleware: Arc<M>,
    ) -> Result<AMM, AMMError<M>> {
        let pair_created_event: PairCreatedFilter =
            PairCreatedFilter::decode_log(&RawLog::from(log))?;
        Ok(AMM::UniswapV2Pool(
            UniswapV2Pool::new_from_address(pair_created_event.pair, self.fee, middleware).await?,
        ))
    }

    fn new_empty_amm_from_log(&self, log: Log) -> Result<AMM, ethers::abi::Error> {
        let pair_created_event = PairCreatedFilter::decode_log(&RawLog::from(log))?;

        Ok(AMM::UniswapV2Pool(UniswapV2Pool {
            address: pair_created_event.pair,
            token_a: pair_created_event.token_0,
            token_b: pair_created_event.token_1,
            token_a_decimals: 0,
            token_b_decimals: 0,
            reserve_0: 0,
            reserve_1: 0,
            fee: 0,
        }))
    }

    async fn get_all_amms<M: 'static + Middleware>(
        &self,
        _to_block: Option<u64>,
        middleware: Arc<M>,
        _step: u64,
    ) -> Result<Vec<AMM>, AMMError<M>> {
        self.get_all_pairs_via_batched_calls(middleware).await
    }

    async fn populate_amm_data<M: Middleware>(
        &self,
        amms: &mut [AMM],
        _block_number: Option<u64>,
        middleware: Arc<M>,
    ) -> Result<(), AMMError<M>> {
        let step = 127; //Max batch size for call
        for amm_chunk in amms.chunks_mut(step) {
            batch_request::get_amm_data_batch_request(amm_chunk, middleware.clone()).await?;
        }
        Ok(())
    }

    fn creation_block(&self) -> u64 {
        self.creation_block
    }
}
