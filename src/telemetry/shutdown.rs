/// Registers a ctrl-c handler to gracefully shutdown the driver
pub fn register_shutdown() {
    ctrlc::set_handler(move || {
        println!();
        tracing::info!(target: "magi", "shutting down...");
        std::process::exit(0);
    })
    .expect("failed to register shutdown handler");
}
