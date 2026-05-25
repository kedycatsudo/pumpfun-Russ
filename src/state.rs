use crate::{config::AppConfig, wallet::LoadedWallet};

#[derive(Debug)]
pub struct AppState {
    pub runtime: RuntimeState,
    pub config: AppConfig,
    pub wallet: LoadedWallet,
}

impl AppState {
    pub fn new(config: AppConfig, wallet: LoadedWallet) -> Self {
        let runtime = RuntimeState {
            app_name: config.runtime.app_name.clone(),
            environment: config.runtime.environment.clone(),
        };

        Self {
            runtime,
            config,
            wallet,
        }
    }
}

#[derive(Debug)]
pub struct RuntimeState {
    pub app_name: String,
    pub environment: String,
}