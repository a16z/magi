use async_trait::async_trait;

#[async_trait]
pub trait AsyncIterator {
    type Item;

    async fn next(&mut self) -> Option<Self::Item>;
}

