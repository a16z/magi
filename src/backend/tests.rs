#[cfg(tests)]
mod tests {
    use super::*;
    use crate::backend::types::{BlockHash, BlockNumber, ConstructedBlock};
    use std::collections::HashMap;

    #[test]
    fn test_db() {
        let mut db = Database::new("/tmp/magi");
        let block = ConstructedBlock::default();
        db.write_block(block.clone()).unwrap();
        let read_block = db.read_block(block.hash.unwrap()).unwrap();
        assert_eq!(block, read_block);
        db.clear().unwrap();
    }
}