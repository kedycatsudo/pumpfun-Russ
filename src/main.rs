mod app;
mod config;
mod errors;
mod logging;
mod network;
mod shutdown;
mod state;
mod wallet;
mod watcher;

#[tokio::main]
async fn main() {
    if let Err(error) = app::run().await {
        eprintln!("fatal error: {error}");
        std::process::exit(1);
    }
}