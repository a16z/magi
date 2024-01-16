use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use ethers::providers::{JsonRpcClient, Provider};
use eyre::Result;
use futures::future::Either;
use futures::join;

use crate::{
    common::{BlockInfo, Epoch},
    derive::state::State,
    engine::PayloadAttributes,
    l1::{utils::get_l1_block_info, L1BlockInfo},
};

pub mod driver;

/// TODO: Support system config updates.
#[async_trait(?Send)]
pub trait SequencingSource {
    /// Returns the next payload attributes to be built (if any) on top of
    /// `parent_l2_block`. If no attributes are ready to be built, returns `None`.
    async fn get_next_attributes(
        &self,
        state: &Arc<RwLock<State>>,
        parent_l2_block: &BlockInfo,
        parent_epoch: &Epoch,
    ) -> Result<Option<PayloadAttributes>>;
}

pub struct Source<T: SequencingPolicy, U: JsonRpcClient> {
    /// The sequencing policy to use to build attributes.
    policy: T,
    /// L1 provider for ad-hoc queries
    provider: Provider<U>,
}

impl<T: SequencingPolicy, U: JsonRpcClient> Source<T, U> {
    pub fn new(policy: T, provider: Provider<U>) -> Self {
        Self { policy, provider }
    }
}

#[async_trait(?Send)]
impl<T: SequencingPolicy, U: JsonRpcClient> SequencingSource for Source<T, U> {
    async fn get_next_attributes(
        &self,
        state: &Arc<RwLock<State>>,
        parent_l2_block: &BlockInfo,
        parent_epoch: &Epoch,
    ) -> Result<Option<PayloadAttributes>> {
        let safe_l2_head = {
            let state = state.read().unwrap();
            state.safe_head
        };
        // Check if we're ready to try building a new payload.
        if !self.policy.is_ready(parent_l2_block, &safe_l2_head) {
            return Ok(None);
        }
        // Get full l1 epoch info.
        let (parent_l1_epoch, next_l1_epoch) = {
            // Acquire read lock on state to get epoch info (if it exists).
            let state = state.read().unwrap();
            (
                state
                    .l1_info_by_hash(parent_epoch.hash)
                    .map(|i| i.block_info.clone()),
                state
                    .l1_info_by_number(parent_epoch.number + 1)
                    .map(|i| i.block_info.clone()),
            )
        };
        // Get l1 epoch info from provider if it doesn't exist in state.
        // TODO: consider using caching e.g. with the cached crate.
        let (parent_l1_epoch, next_l1_epoch) = join!(
            match parent_l1_epoch {
                Some(info) => Either::Left(async { Ok(info) }),
                None => Either::Right(get_l1_block_info(parent_epoch.hash, &self.provider)),
            },
            match next_l1_epoch {
                Some(info) => Either::Left(async { Ok(info) }),
                None => Either::Right(get_l1_block_info(parent_epoch.number + 1, &self.provider)),
            },
        );
        // TODO: handle recoverable errors, if any.
        // Get next payload attributes and build the payload.
        Ok(Some(
            self.policy
                .get_attributes(
                    parent_l2_block,
                    &parent_l1_epoch?,
                    next_l1_epoch.ok().as_ref(),
                )
                .await?,
        ))
    }
}

#[async_trait]
pub trait SequencingPolicy {
    /// Returns true only if the policy is ready to build a payload on top of `parent_l2_block`.
    fn is_ready(&self, parent_l2_block: &BlockInfo, safe_l2_head: &BlockInfo) -> bool;
    /// Returns the attributes for a payload to be built on top of `parent_l2_block`.
    /// If `next_l1_epoch` is `None`, `parent_l1_epoch` is attempted to be used as the epoch.
    /// However, if it's too late to use `parent_l1_epoch` as the epoch, an error is returned.
    async fn get_attributes(
        &self,
        parent_l2_block: &BlockInfo,
        parent_l1_epoch: &L1BlockInfo,
        next_l1_epoch: Option<&L1BlockInfo>,
    ) -> Result<PayloadAttributes>;
}
