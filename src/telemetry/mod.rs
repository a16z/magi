#![deny(missing_docs, missing_debug_implementations)]

//! Telemetry module
//!
//! This module encompasses telemetry for `magi`.
//! Core components are described below.
//!
//! ### Logging
//!
//! Logging is constructed using the [tracing](https://crates.io/crates/tracing) crate.
//! The `tracing` crate is a framework for instrumenting Rust programs to collect
//! structured, event-based diagnostic information. You can use the [logging::init] function
//! to initialize a global logger, passing in a boolean `verbose` parameter. This function
//! will return an error if a logger has already been initialized.
//!
//! ### Shutdown
//!
//! The shutdown module provides a [shutdown::register_shutdown] function which will
//! register a ctrl-c handler to gracefully shutdown the running thread.

/// The Logging Module
pub mod logging;

/// The Shutdown Module
pub mod shutdown;

// Re-export inner modules
pub use logging::*;
pub use shutdown::*;

/// Export a prelude to re-export common traits and types
pub mod prelude {
    pub use super::*;
    pub use tracing::{debug, error, info, span, trace, warn, Level};
    pub use tracing_subscriber::{fmt, prelude::*};
}
