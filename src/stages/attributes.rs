use std::{cell::RefCell, rc::Rc};

use ethers::types::{Address, H256};

use super::{
    batches::{Batch, Batches, RawTransaction},
    Stage,
};

pub struct Attributes {
    prev_stage: Rc<RefCell<Batches>>,
}

impl Stage for Attributes {
    type Output = PayloadAttributes;

    fn next(&mut self) -> eyre::Result<Option<Self::Output>> {
        Ok(if let Some(batch) = self.prev_stage.borrow_mut().next()? {
            Some(self.derive_attributes(batch))
        } else {
            None
        })
    }
}

impl Attributes {
    pub fn new(prev_stage: Rc<RefCell<Batches>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { prev_stage }))
    }

    fn derive_attributes(&self, batch: Batch) -> PayloadAttributes {
        PayloadAttributes {
            timestamp: 0,
            random: H256::default(),
            suggested_fee_recipient: Address::default(),
            transactions: batch.transactions,
        }
    }
}

#[derive(Debug)]
pub struct PayloadAttributes {
    pub timestamp: u64,
    pub random: H256,
    pub suggested_fee_recipient: Address,
    pub transactions: Vec<RawTransaction>,
}
