use tracing::info;

pub async fn wait_for_signal() {
    info!("waiting for shutdown signal");
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}