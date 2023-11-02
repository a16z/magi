use ethers::{
    providers::{JsonRpcClient, Middleware, Provider},
    types::{Block, BlockId, H256},
};
use eyre::{Result, WrapErr};

use super::L1BlockInfo;

/// Fetches the l1 block info for `block_id` (which can be either a block number or a block hash), using `provider`.
pub async fn get_l1_block_info<T: Into<BlockId> + Send + Sync, U: JsonRpcClient>(
    block_id: T,
    provider: &Provider<U>,
) -> Result<L1BlockInfo> {
    let block = provider.get_block(block_id).await;
    block
        .wrap_err_with(|| "failed to get l1 block")
        .and_then(|b| b.ok_or(eyre::eyre!("no l1 block found")))
        .and_then(|b| try_create_l1_block_info(&b))
}

/// Tries to extract l1 block info from `block`.
fn try_create_l1_block_info(block: &Block<H256>) -> Result<L1BlockInfo> {
    Ok(L1BlockInfo {
        number: block
            .number
            .ok_or(eyre::eyre!("block number missing"))?
            .as_u64(),
        hash: block.hash.ok_or(eyre::eyre!("block hash missing"))?,
        timestamp: block.timestamp.as_u64(),
        base_fee: block
            .base_fee_per_gas
            .ok_or(eyre::eyre!("base fee missing"))?,
        mix_hash: block.mix_hash.ok_or(eyre::eyre!("mix_hash missing"))?,
    })
}
