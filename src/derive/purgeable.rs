/// Iterator that can purge itself
pub trait PurgeableIterator: Iterator {
    fn purge(&mut self);
}
