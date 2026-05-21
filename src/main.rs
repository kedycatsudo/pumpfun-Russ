mod app;
mod config;
mod errors;
mod logging;
mod shutdown;
mod state;
mod wallet;

#[tokio::main]
async fn main() {
    if let Err(error) = app::run().await {
        eprintln!("fatal error: {error}");
        std::process::exit(1);
    }
}