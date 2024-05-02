//! Telemetry module
//!
//! This module encompasses telemetry and logging.
//! Core components are described below.
//!
//! ### Logging
//!
//! Logging is constructed using the [tracing](https://crates.io/crates/tracing) crate.
//! The `tracing` crate is a framework for instrumenting Rust programs to collect
//! structured, event-based diagnostic information. You can use the [crate::telemetry::init] function
//! to initialize a global logger, passing in a boolean `verbose` parameter. This function
//! will return an error if a logger has already been initialized.
//!
//! ### Metrics
//!
//! Metrics are collected using the [prometheus](https://crates.io/crates/prometheus) crate.

pub mod logging;
pub use logging::{
    build_subscriber, get_rolling_file_appender, get_rotation_strategy, init, AnsiTermLayer,
    AnsiVisitor, DEFAULT_ROTATION, LOG_FILE_NAME_PREFIX,
};

pub mod metrics;
pub use metrics::{init as init_metrics, FINALIZED_HEAD, SAFE_HEAD, SYNCED};

/// Contains common telemetry and logging types.
/// Re-exports [tracing] and [tracing_subscriber] items.
pub mod prelude {
    pub use super::*;
    pub use tracing::{debug, error, info, span, trace, warn, Level};
    pub use tracing_subscriber::{fmt, prelude::*};
}
