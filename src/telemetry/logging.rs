use std::env::current_dir;
use std::path::Path;

use eyre::Result;
use tracing::subscriber::set_global_default;
use tracing::{Level, Subscriber};
use tracing_log::LogTracer;
use tracing_subscriber::Layer;
use tracing_subscriber::{layer::SubscriberExt, EnvFilter, Registry};

use ansi_term::Colour::{Blue, Cyan, Purple, Red, Yellow};

/// Configure logging telemetry
pub fn init(verbose: bool, target: Option<String>) -> Result<()> {
    let constructed_target = match verbose {
        true => "magi=debug",
        false => "magi=info",
    };
    let target = target.unwrap_or(constructed_target.to_string());
    let subscriber = get_subscriber(target);
    init_subscriber(subscriber)
}

/// Subscriber Composer
///
/// Builds a subscriber with multiple layers into a [tracing](https://crates.io/crates/tracing) subscriber.
pub fn get_subscriber(env_filter: String) -> impl Subscriber + Sync + Send {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));
    let formatting_layer = AsniTermLayer;
    Registry::default().with(env_filter).with(formatting_layer)
}

/// Globally registers a subscriber.
/// This will error if a subscriber has already been registered.
pub fn init_subscriber(subscriber: impl Subscriber + Send + Sync) -> Result<()> {
    LogTracer::init().map_err(|_| eyre::eyre!("Failed to set logger"))?;
    set_global_default(subscriber).map_err(|_| eyre::eyre!("Failed to set subscriber"))
}

/// The AnsiVisitor
#[derive(Debug)]
pub struct AnsiVisitor;

impl tracing::field::Visit for AnsiVisitor {
    fn record_f64(&mut self, _: &tracing::field::Field, value: f64) {
        println!("{value}")
    }

    fn record_i64(&mut self, _: &tracing::field::Field, value: i64) {
        println!("{value}")
    }

    fn record_u64(&mut self, _: &tracing::field::Field, value: u64) {
        println!("{value}")
    }

    fn record_bool(&mut self, _: &tracing::field::Field, value: bool) {
        println!("{value}")
    }

    fn record_str(&mut self, _: &tracing::field::Field, value: &str) {
        println!("{value}")
    }

    fn record_error(
        &mut self,
        _: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        println!("{value}")
    }

    fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        println!("{value:?}")
    }
}

/// An Ansi Term layer for tracing
#[derive(Debug)]
pub struct AsniTermLayer;

impl<S> Layer<S> for AsniTermLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        // Print the timestamp
        let utc: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
        print!("[{}] ", Cyan.paint(utc.to_rfc2822()));

        // Print the level prefix
        match *event.metadata().level() {
            Level::ERROR => {
                eprint!("{}: ", Red.paint("ERROR"));
            }
            Level::WARN => {
                print!("{}: ", Yellow.paint("WARN"));
            }
            Level::INFO => {
                print!("{}: ", Blue.paint("INFO"));
            }
            Level::DEBUG => {
                print!("DEBUG: ");
            }
            Level::TRACE => {
                print!("{}: ", Purple.paint("TRACE"));
            }
        }

        print!("{} ", Purple.paint(event.metadata().target()));

        let original_location = event
            .metadata()
            .name()
            .split(' ')
            .last()
            .unwrap_or_default();
        let relative_path = current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Remove common prefixes from the location and relative path
        let location_path = std::path::Path::new(original_location);
        let relative_path_path = std::path::Path::new(&relative_path);
        let common_prefix = location_path
            .ancestors()
            .collect::<Vec<&Path>>()
            .iter()
            .cloned()
            .rev()
            .zip(
                relative_path_path
                    .ancestors()
                    .collect::<Vec<&Path>>()
                    .iter()
                    .cloned()
                    .rev(),
            )
            .take_while(|(a, b)| a == b)
            .last()
            .map(|(a, _)| a)
            .unwrap_or_else(|| std::path::Path::new(""));
        let location = location_path
            .strip_prefix(common_prefix)
            .unwrap_or(location_path)
            .to_str()
            .unwrap_or(original_location);
        let location = location.strip_prefix('/').unwrap_or(location);
        print!("at {} ", Cyan.paint(location.to_string()));

        let mut visitor = AnsiVisitor;
        event.record(&mut visitor);
    }
}
