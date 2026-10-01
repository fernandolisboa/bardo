//! Secret stores for provider keys. Keys never touch the database or any
//! file Bardo writes (ADR-0001).

use std::collections::HashMap;
use std::sync::Mutex;

use bardo_domain::{ApiKey, ProfileId, Provider, SecretStore, SecretStoreError};

/// The secret store of this platform: Windows Credential Manager on
/// Windows. Elsewhere (development builds on Linux or macOS) keys are kept
/// in memory and forgotten when the app closes.
pub fn platform_secret_store() -> Result<Box<dyn SecretStore>, SecretStoreError> {
    #[cfg(windows)]
    {
        Ok(Box::new(CredentialManager::new()?))
    }
    #[cfg(not(windows))]
    {
        Ok(Box::new(MemorySecretStore::default()))
    }
}

/// Keys in process memory only. For tests and non-Windows development.
#[derive(Default)]
pub struct MemorySecretStore {
    keys: Mutex<HashMap<(ProfileId, Provider), ApiKey>>,
}

impl MemorySecretStore {
    fn keys(&self) -> std::sync::MutexGuard<'_, HashMap<(ProfileId, Provider), ApiKey>> {
        self.keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SecretStore for MemorySecretStore {
    fn get(
        &self,
        owner: ProfileId,
        provider: Provider,
    ) -> Result<Option<ApiKey>, SecretStoreError> {
        Ok(self.keys().get(&(owner, provider)).cloned())
    }

    fn set(
        &self,
        owner: ProfileId,
        provider: Provider,
        key: &ApiKey,
    ) -> Result<(), SecretStoreError> {
        self.keys().insert((owner, provider), key.clone());
        Ok(())
    }

    fn delete(&self, owner: ProfileId, provider: Provider) -> Result<(), SecretStoreError> {
        self.keys().remove(&(owner, provider));
        Ok(())
    }
}

/// Name of the credential holding a profile's key for a provider, as
/// Windows Credential Manager lists it (under "Generic Credentials").
pub fn credential_target(owner: ProfileId, provider: Provider) -> String {
    format!("Bardo/{owner}/{}", provider.code())
}

/// A secret store failure, described without the failing value: some
/// platform errors carry the raw secret bytes, so they are never wrapped.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
#[cfg_attr(not(windows), allow(dead_code))]
struct StoreFailure(String);

#[cfg(windows)]
pub use credential_manager::CredentialManager;

#[cfg(windows)]
mod credential_manager {
    use std::collections::HashMap;
    use std::sync::Arc;

    use bardo_domain::{ApiKey, ProfileId, Provider, SecretStore, SecretStoreError};
    use keyring_core::Entry;
    use keyring_core::api::CredentialStoreApi as _;
    use windows_native_keyring_store::Store;

    use super::{StoreFailure, credential_target};

    /// Generic credentials in Windows Credential Manager, readable only by
    /// the signed-in Windows user. Persistence is local to this computer:
    /// keys do not roam with a domain profile.
    pub struct CredentialManager {
        store: Arc<Store>,
    }

    impl CredentialManager {
        pub fn new() -> Result<Self, SecretStoreError> {
            let store = Store::new().map_err(|error| failure("open", &error))?;
            Ok(Self { store })
        }

        fn entry(&self, owner: ProfileId, provider: Provider) -> Result<Entry, SecretStoreError> {
            let target = credential_target(owner, provider);
            let modifiers = HashMap::from([("target", target.as_str()), ("persistence", "Local")]);
            let owner = owner.to_string();
            self.store
                .build(provider.code(), &owner, Some(&modifiers))
                .map_err(|error| failure("address", &error))
        }
    }

    impl SecretStore for CredentialManager {
        fn get(
            &self,
            owner: ProfileId,
            provider: Provider,
        ) -> Result<Option<ApiKey>, SecretStoreError> {
            match self.entry(owner, provider)?.get_password() {
                Ok(stored) => ApiKey::parse(provider, &stored).map(Some).map_err(|error| {
                    SecretStoreError(Box::new(StoreFailure(format!(
                        "the stored {provider} key is not usable: {error}"
                    ))))
                }),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(error) => Err(failure("read", &error)),
            }
        }

        fn set(
            &self,
            owner: ProfileId,
            provider: Provider,
            key: &ApiKey,
        ) -> Result<(), SecretStoreError> {
            self.entry(owner, provider)?
                .set_password(key.expose())
                .map_err(|error| failure("write", &error))
        }

        fn delete(&self, owner: ProfileId, provider: Provider) -> Result<(), SecretStoreError> {
            match self.entry(owner, provider)?.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(error) => Err(failure("delete", &error)),
            }
        }
    }

    /// Keeps only errors that cannot carry the secret; the rest become a
    /// fixed description.
    fn failure(action: &str, error: &keyring_core::Error) -> SecretStoreError {
        use keyring_core::Error;
        let cause = match error {
            Error::PlatformFailure(_)
            | Error::NoStorageAccess(_)
            | Error::NoEntry
            | Error::TooLong(..)
            | Error::Invalid(..)
            | Error::NoDefaultStore
            | Error::NotSupportedByStore(_) => error.to_string(),
            Error::BadEncoding(_) | Error::BadDataFormat(..) => {
                "the stored secret is malformed".to_owned()
            }
            _ => "unexpected credential store error".to_owned(),
        };
        SecretStoreError(Box::new(StoreFailure(format!(
            "could not {action} the credential: {cause}"
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(provider: Provider, text: &str) -> ApiKey {
        ApiKey::parse(provider, text).unwrap()
    }

    /// The contract every secret store keeps. Run against each adapter.
    fn contract(store: &dyn SecretStore) {
        let owner = ProfileId::new();
        let other = ProfileId::new();
        let claude = key(Provider::Claude, "sk-ant-test-0001");

        assert_eq!(store.get(owner, Provider::Claude).unwrap(), None);

        store.set(owner, Provider::Claude, &claude).unwrap();
        assert_eq!(store.get(owner, Provider::Claude).unwrap(), Some(claude));
        assert_eq!(store.get(owner, Provider::Gemini).unwrap(), None);
        assert_eq!(store.get(other, Provider::Claude).unwrap(), None);

        let replacement = key(Provider::Claude, "sk-ant-test-0002");
        store.set(owner, Provider::Claude, &replacement).unwrap();
        assert_eq!(
            store.get(owner, Provider::Claude).unwrap(),
            Some(replacement)
        );

        store.delete(owner, Provider::Claude).unwrap();
        assert_eq!(store.get(owner, Provider::Claude).unwrap(), None);
        store.delete(owner, Provider::Claude).unwrap();
    }

    #[test]
    fn memory_store_keeps_the_contract() {
        contract(&MemorySecretStore::default());
    }

    #[test]
    fn credential_names_are_per_profile_and_provider() {
        let owner = ProfileId::new();
        assert_eq!(
            credential_target(owner, Provider::YouTubeData),
            format!("Bardo/{owner}/youtube-data")
        );
    }

    /// Writes real credentials for a throwaway profile and removes them.
    #[cfg(windows)]
    #[test]
    fn credential_manager_keeps_the_contract() {
        contract(&CredentialManager::new().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn credential_manager_round_trips_a_compound_key() {
        let store = CredentialManager::new().unwrap();
        let owner = ProfileId::new();
        let higgsfield = key(Provider::Higgsfield, "key-id-0001:secret-0001");
        store.set(owner, Provider::Higgsfield, &higgsfield).unwrap();
        let read = store.get(owner, Provider::Higgsfield);
        store.delete(owner, Provider::Higgsfield).unwrap();
        assert_eq!(read.unwrap(), Some(higgsfield));
    }
}
