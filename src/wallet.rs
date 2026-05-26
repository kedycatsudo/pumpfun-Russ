use std::path::Path;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
};

use crate::errors::WalletError;

#[derive(Debug, Clone)]
pub struct LoadedWallet {
    pub keypair_path: String,
    pub pubkey: Pubkey,
    pub pubkey_short: String,
}

impl LoadedWallet {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, WalletError> {
        let path_ref = path.as_ref();

        let keypair = read_keypair_file(path_ref).map_err(|source| WalletError::Parse {
            path: path_ref.display().to_string(),
            message: source.to_string(),
        })?;

        let pubkey = keypair.pubkey();
        let pubkey_short = shorten_pubkey(&pubkey);

        Ok(Self {
            keypair_path: path_ref.display().to_string(),
            pubkey,
            pubkey_short,
        })
    }
}

fn shorten_pubkey(pubkey: &Pubkey) -> String {
    let value = pubkey.to_string();

    if value.len() <= 10 {
        return value;
    }

    format!("{}...{}", &value[..4], &value[value.len() - 4..])
}