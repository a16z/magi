use std::net::SocketAddr;

pub mod handlers;
pub mod service;
pub mod signer;

pub const LISTENING_AS_STR: &str = "0.0.0.0:9876";

lazy_static::lazy_static! {
    pub static ref LISTENING: SocketAddr = LISTENING_AS_STR.parse().unwrap();
}
