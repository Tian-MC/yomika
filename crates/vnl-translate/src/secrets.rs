//! API-key storage.
//!
//! Primary: the freedesktop Secret Service (gnome-keyring, KWallet ≥ 5.97,
//! KeePassXC) through `keyring-core` + `zbus-secret-service-keyring-store`.
//! Entries use service `vn-launcher` and the provider id as user name, so
//! they show up in Seahorse as "vn-launcher / <provider>".
//!
//! Fallback (only when no Secret Service is reachable, or when
//! `VNL_SECRET_BACKEND=file`): `<config_dir>/translation-secrets.json`
//! created with mode 0600. The active backend is reported in
//! `ProviderInfo.notes` so the user knows where the key lives.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vnl_core::{AppError, ErrorCode};

// Keep existing provider credentials accessible after the product rename.
pub const KEYRING_SERVICE: &str = "vn-launcher";

pub trait SecretStore: Send + Sync {
    /// Short human description for diagnostics.
    fn describe(&self) -> String;
    fn get(&self, key: &str) -> Result<Option<String>, AppError>;
    fn set(&self, key: &str, secret: &str) -> Result<(), AppError>;
    fn delete(&self, key: &str) -> Result<(), AppError>;
    /// True when secrets are protected by the desktop keyring.
    fn is_secure(&self) -> bool;
}

/// Runs a blocking keyring call on a fresh OS thread: the zbus blocking API
/// starts its own Tokio runtime and must not run on a runtime worker thread.
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    std::thread::spawn(f).join().map_err(|_| AppError::internal("keyring thread panicked"))
}

fn keyring_err(e: keyring_core::Error) -> AppError {
    AppError::new(ErrorCode::Internal, format!("Secret Service error: {e}"))
}

pub struct KeyringSecretStore {
    store: Arc<zbus_secret_service_keyring_store::Store>,
    service: String,
}

impl KeyringSecretStore {
    /// Connects to the Secret Service and performs a read-only probe.
    pub fn connect(service: &str) -> Result<Self, AppError> {
        let service = service.to_string();
        off_runtime(move || -> Result<Self, AppError> {
            let store = zbus_secret_service_keyring_store::Store::new().map_err(keyring_err)?;
            let me = Self { store, service };
            // Probe: a lookup must either succeed or report NoEntry.
            match me.entry("__probe__")?.get_password() {
                Ok(_) | Err(keyring_core::Error::NoEntry) => Ok(me),
                Err(e) => Err(keyring_err(e)),
            }
        })?
    }

    fn entry(&self, key: &str) -> Result<keyring_core::Entry, AppError> {
        use keyring_core::api::CredentialStoreApi;
        self.store.build(&self.service, key, None).map_err(keyring_err)
    }
}

impl SecretStore for KeyringSecretStore {
    fn describe(&self) -> String {
        "API key stored in the desktop keyring (Secret Service)".into()
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        let entry = self.entry(key)?;
        off_runtime(move || match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        })?
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), AppError> {
        let entry = self.entry(key)?;
        let secret = secret.to_string();
        off_runtime(move || entry.set_password(&secret).map_err(keyring_err))?
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        let entry = self.entry(key)?;
        off_runtime(move || match entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        })?
    }

    fn is_secure(&self) -> bool {
        true
    }
}

/// Plain JSON file readable only by the user (0600). Fallback only.
pub struct FileSecretStore {
    path: PathBuf,
    lock: Mutex<()>,
    reason: String,
}

impl FileSecretStore {
    pub fn new(path: PathBuf, reason: impl Into<String>) -> Self {
        Self { path, lock: Mutex::new(()), reason: reason.into() }
    }

    fn read_all(&self) -> Result<BTreeMap<String, String>, AppError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                AppError::new(ErrorCode::Io, format!("corrupt secrets file {}: {e}", self.path.display()))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_all(&self, map: &BTreeMap<String, String>) -> Result<(), AppError> {
        use std::io::Write;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            f.write_all(&serde_json::to_vec_pretty(map).map_err(|e| AppError::internal(e.to_string()))?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

impl SecretStore for FileSecretStore {
    fn describe(&self) -> String {
        format!(
            "API key stored in {} (permissions 0600) because the desktop keyring is unavailable: {}",
            self.path.display(),
            self.reason
        )
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        Ok(self.read_all()?.get(key).cloned())
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), AppError> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut map = self.read_all()?;
        map.insert(key.to_string(), secret.to_string());
        self.write_all(&map)
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut map = self.read_all()?;
        if map.remove(key).is_some() {
            self.write_all(&map)?;
        }
        Ok(())
    }

    fn is_secure(&self) -> bool {
        false
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemorySecretStore {
    map: Mutex<BTreeMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn describe(&self) -> String {
        "API key kept in memory (test store)".into()
    }
    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), AppError> {
        self.map.lock().unwrap().insert(key.into(), secret.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), AppError> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
    fn is_secure(&self) -> bool {
        false
    }
}

/// Keyring when reachable, otherwise the 0600 file in `config_dir`.
/// `VNL_SECRET_BACKEND=file` forces the file store (portable installs).
pub fn default_secret_store(config_dir: &std::path::Path) -> Arc<dyn SecretStore> {
    let file = config_dir.join("translation-secrets.json");
    if std::env::var("VNL_SECRET_BACKEND").map(|v| v == "file").unwrap_or(false) {
        return Arc::new(FileSecretStore::new(file, "VNL_SECRET_BACKEND=file is set"));
    }
    match KeyringSecretStore::connect(KEYRING_SERVICE) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            tracing::warn!(error = %e, "Secret Service unavailable; falling back to a 0600 secrets file");
            Arc::new(FileSecretStore::new(file, e.message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_roundtrip_with_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cfg/secrets.json");
        let store = FileSecretStore::new(path.clone(), "test");
        assert_eq!(store.get("deepl").unwrap(), None);
        store.set("deepl", "abc:fx").unwrap();
        store.set("openai-compatible", "sk-1").unwrap();
        assert_eq!(store.get("deepl").unwrap().as_deref(), Some("abc:fx"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        store.delete("deepl").unwrap();
        assert_eq!(store.get("deepl").unwrap(), None);
        assert_eq!(store.get("openai-compatible").unwrap().as_deref(), Some("sk-1"));
        assert!(!store.is_secure());
        assert!(store.describe().contains("0600"));
    }

    /// Touches the real desktop keyring (writes and deletes one dummy entry
    /// under service `yomika-test`). Run manually with `-- --ignored`.
    #[test]
    #[ignore]
    fn real_secret_service_roundtrip() {
        let store = KeyringSecretStore::connect("yomika-test").expect("Secret Service reachable");
        store.set("dummy-provider", "not-a-real-key").unwrap();
        assert_eq!(store.get("dummy-provider").unwrap().as_deref(), Some("not-a-real-key"));
        store.delete("dummy-provider").unwrap();
        assert_eq!(store.get("dummy-provider").unwrap(), None);
    }
}
