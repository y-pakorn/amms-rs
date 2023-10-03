use ethers::prelude::{AbiError, ContractError};
use ethers::providers::{Middleware, ProviderError};
use ethers::types::{H160, U256};
use std::time::SystemTimeError;
use thiserror::Error;
use tokio::task::JoinError;
use uniswap_v3_math::error::UniswapV3MathError;

#[derive(Error, Debug)]
pub enum AMMError<M>
where
    M: Middleware,
{
    #[error("Middleware error: {0}")]
    MiddlewareError(<M as Middleware>::Error),
    #[error("Provider error: {0}")]
    ProviderError(#[from] ProviderError),
    #[error("Contract error: {0}")]
    ContractError(#[from] ContractError<M>),
    #[error("ABI Codec error: {0}")]
    ABICodecError(#[from] AbiError),
    #[error("Eth ABI error: {0}")]
    EthABIError(#[from] ethers::abi::Error),
    #[error("Join error: {0}")]
    JoinError(#[from] JoinError),
    #[error("Serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Error when converting from hex to U256")]
    FromHexError,
    #[error("Uniswap V3 math error: {0}")]
    UniswapV3MathError(#[from] UniswapV3MathError),
    #[error("Pair for token_a {0}/token_b {1} does not exist in provided dexes")]
    PairDoesNotExistInDexes(H160, H160),
    #[error("Could not initialize new pool from event log")]
    UnrecognizedPoolCreatedEventLog,
    #[error("Error when syncing pool {0}")]
    SyncError(H160),
    #[error("Error when getting pool data")]
    PoolDataError,
    #[error("Arithmetic error: {0}")]
    ArithmeticError(#[from] ArithmeticError),
    #[error("No initialized ticks during v3 swap simulation")]
    NoInitializedTicks,
    #[error("No liquidity net found during v3 swap simulation")]
    NoLiquidityNet,
    #[error("Incongruent AMMS supplied to batch request")]
    IncongruentAMMs,
    #[error("Invalid ERC4626 fee")]
    InvalidERC4626Fee,
    #[error("Event log error: {0}")]
    EventLogError(#[from] EventLogError),
    #[error("Block number not found")]
    BlockNumberNotFound,
    #[error("Swap simulation error: {0}")]
    SwapSimulationError(#[from] SwapSimulationError),
    #[error("Invalid data from batch request {0}")]
    BatchRequestError(H160),
    #[error("Checkpoint error: {0}")]
    CheckpointError(#[from] CheckpointError),
}

#[derive(Error, Debug)]
pub enum ArithmeticError {
    #[error("Shadow overflow: {0}")]
    ShadowOverflow(U256),
    #[error("Rounding Error")]
    RoundingError,
    #[error("Y is zero")]
    YIsZero,
    #[error("Sqrt price overflow")]
    SqrtPriceOverflow,
    #[error("U128 conversion error")]
    U128ConversionError,
    #[error("Uniswap v3 math error: {0}")]
    UniswapV3MathError(#[from] UniswapV3MathError),
}

#[derive(Error, Debug)]
pub enum EventLogError {
    #[error("Invalid event signature")]
    InvalidEventSignature,
    #[error("Log Block number not found")]
    LogBlockNumberNotFound,
    #[error("Eth abi error: {0}")]
    EthABIError(#[from] ethers::abi::Error),
    #[error("ABI error: {0}")]
    ABIError(#[from] AbiError),
}

#[derive(Error, Debug)]
pub enum SwapSimulationError {
    #[error("Could not get next tick")]
    InvalidTick,
    #[error("Uniswap v3 math error: {0}")]
    UniswapV3MathError(#[from] UniswapV3MathError),
    #[error("Liquidity underflow")]
    LiquidityUnderflow,
}

#[derive(Error, Debug)]
pub enum CheckpointError {
    #[error("System time error: {0}")]
    SystemTimeError(#[from] SystemTimeError),
    #[error("Serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
}
