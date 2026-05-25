use std::{fs, path::Path};

use crate::errors::WalletError;

#[derive(Debug)]
pub struct LoadedWallet {
    pub keypair_path: String,
    pub raw_contents: String,
}

impl LoadedWallet {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, WalletError> {
        let path_ref = path.as_ref();

        let raw_contents = fs::read_to_string(path_ref).map_err(|source| WalletError::Read {
            path: path_ref.display().to_string(),
            source,
        })?;

        if raw_contents.trim().is_empty() {
            return Err(WalletError::Empty {
                path: path_ref.display().to_string(),
            });
        }

        Ok(Self {
            keypair_path: path_ref.display().to_string(),
            raw_contents,
        })
    }
}