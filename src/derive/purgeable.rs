use crate::derive::async_iterator::AsyncIterator;

/// Iterator that can purge itself
pub trait PurgeableIterator: Iterator {
    fn purge(&mut self);
}

/// AsyncIterator that can purge itself
pub trait PurgeableAsyncIterator: AsyncIterator {
    fn purge(&mut self);
}
