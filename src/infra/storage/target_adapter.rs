use crate::error::Result;
use crate::infra::storage::atomic_fs::atomic_write;
use crate::infra::storage::paths::{get_active_google_accounts_path, get_active_oauth_creds_path};
use crate::ports::target::TargetPort;
use std::fs;

#[derive(Default, Clone)]
pub struct TargetAdapter;

impl TargetPort for TargetAdapter {
    fn read_oauth_creds(&self) -> Result<Option<Vec<u8>>> {
        let path = get_active_oauth_creds_path()?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read(path)?))
    }

    fn read_google_accounts(&self) -> Result<Option<Vec<u8>>> {
        let path = get_active_google_accounts_path()?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read(path)?))
    }

    fn write_oauth_creds(&self, data: &[u8]) -> Result<()> {
        let path = get_active_oauth_creds_path()?;
        atomic_write(path, data)
    }

    fn write_google_accounts(&self, data: &[u8]) -> Result<()> {
        let path = get_active_google_accounts_path()?;
        atomic_write(path, data)
    }

    fn delete_oauth_creds(&self) -> Result<()> {
        let path = get_active_oauth_creds_path()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn delete_google_accounts(&self) -> Result<()> {
        let path = get_active_google_accounts_path()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn active_exists(&self) -> bool {
        let oauth_ok = get_active_oauth_creds_path()
            .map(|p| p.exists())
            .unwrap_or(false);
        let accounts_ok = get_active_google_accounts_path()
            .map(|p| p.exists())
            .unwrap_or(false);
        oauth_ok || accounts_ok
    }
}
