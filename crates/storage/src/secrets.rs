//! Secret stores for provider keys, network app credentials and OAuth
//! tokens. None of them touches the database or any file Bardo writes
//! (ADR-0001, ADR-0008).

use std::collections::HashMap;
use std::sync::Mutex;

use bardo_domain::{
    ApiKey, AppCredentials, ConnectionSecrets, Network, NetworkAccountId, ProfileId, Provider,
    SecretStore, SecretStoreError, TokenSet,
};
use zeroize::Zeroizing;

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

/// Where this platform keeps network app credentials and OAuth tokens: the
/// same store as provider keys.
pub fn platform_connection_secrets() -> Result<Box<dyn ConnectionSecrets>, SecretStoreError> {
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
/// Keeps Credential Manager's size limit, so tests see what Windows does.
#[derive(Default)]
pub struct MemorySecretStore {
    keys: Mutex<HashMap<(ProfileId, Provider), ApiKey>>,
    /// App credentials and tokens, by credential name.
    blobs: Mutex<HashMap<String, Zeroizing<String>>>,
}

impl MemorySecretStore {
    fn keys(&self) -> std::sync::MutexGuard<'_, HashMap<(ProfileId, Provider), ApiKey>> {
        self.keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl MemorySecretStore {
    fn blobs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Zeroizing<String>>> {
        self.blobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn put(&self, target: String, value: Zeroizing<String>) -> Result<(), SecretStoreError> {
        if value.len() > MAX_BLOB_BYTES {
            return Err(too_long(value.len()));
        }
        self.blobs().insert(target, value);
        Ok(())
    }
}

impl ConnectionSecrets for MemorySecretStore {
    fn app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<Option<AppCredentials>, SecretStoreError> {
        self.blobs()
            .get(&app_credentials_target(owner, network))
            .map(|stored| {
                AppCredentials::decode(network, stored).ok_or_else(|| malformed("app credentials"))
            })
            .transpose()
    }

    fn set_app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
        credentials: &AppCredentials,
    ) -> Result<(), SecretStoreError> {
        self.put(app_credentials_target(owner, network), credentials.encode())
    }

    fn delete_app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<(), SecretStoreError> {
        self.blobs().remove(&app_credentials_target(owner, network));
        Ok(())
    }

    fn tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
    ) -> Result<Option<TokenSet>, SecretStoreError> {
        self.blobs()
            .get(&tokens_target(owner, account))
            .map(|stored| TokenSet::decode(stored).ok_or_else(|| malformed("tokens")))
            .transpose()
    }

    fn set_tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
        tokens: &TokenSet,
    ) -> Result<(), SecretStoreError> {
        let encoded = tokens
            .encode()
            .map_err(|error| SecretStoreError(Box::new(error)))?;
        self.put(tokens_target(owner, account), encoded)
    }

    fn delete_tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
    ) -> Result<(), SecretStoreError> {
        self.blobs().remove(&tokens_target(owner, account));
        Ok(())
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

/// Name of the credential holding a profile's app credentials for a
/// network.
pub fn app_credentials_target(owner: ProfileId, network: Network) -> String {
    format!("Bardo/{owner}/app/{}", network.code())
}

/// Name of the credential holding a network account's OAuth tokens.
pub fn tokens_target(owner: ProfileId, account: NetworkAccountId) -> String {
    format!("Bardo/{owner}/tokens/{account}")
}

/// What one generic Windows credential holds
/// (`CRED_MAX_CREDENTIAL_BLOB_SIZE`). App credentials and tokens are stored
/// as UTF-8 bytes, so this is their limit in bytes.
const MAX_BLOB_BYTES: usize = TokenSet::MAX_STORED_BYTES;

fn too_long(bytes: usize) -> SecretStoreError {
    SecretStoreError(Box::new(StoreFailure(format!(
        "the secret is {bytes} bytes, over the {MAX_BLOB_BYTES} a credential holds"
    ))))
}

fn malformed(what: &str) -> SecretStoreError {
    SecretStoreError(Box::new(StoreFailure(format!(
        "the stored {what} are malformed"
    ))))
}

/// A secret store failure, described without the failing value: some
/// platform errors carry the raw secret bytes, so they are never wrapped.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct StoreFailure(String);

#[cfg(windows)]
pub use credential_manager::CredentialManager;

#[cfg(windows)]
mod credential_manager {
    use std::collections::HashMap;
    use std::sync::Arc;

    use bardo_domain::{
        ApiKey, AppCredentials, ConnectionSecrets, Network, NetworkAccountId, ProfileId, Provider,
        SecretStore, SecretStoreError, TokenSet,
    };
    use keyring_core::Entry;
    use keyring_core::api::CredentialStoreApi as _;
    use windows_native_keyring_store::Store;
    use zeroize::Zeroizing;

    use super::{
        MAX_BLOB_BYTES, StoreFailure, app_credentials_target, credential_target, malformed,
        tokens_target, too_long,
    };

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

        /// A credential addressed by its full name. `service` groups them
        /// for the store; the target is what Credential Manager lists.
        fn named(&self, target: &str, service: &str) -> Result<Entry, SecretStoreError> {
            let modifiers = HashMap::from([("target", target), ("persistence", "Local")]);
            self.store
                .build(service, target, Some(&modifiers))
                .map_err(|error| failure("address", &error))
        }

        /// The UTF-8 text stored under `target`. Stored as bytes rather
        /// than a password, which Windows keeps as UTF-16 and so halves
        /// the room.
        fn read(
            &self,
            target: &str,
            service: &str,
        ) -> Result<Option<Zeroizing<String>>, SecretStoreError> {
            match self.named(target, service)?.get_secret() {
                Ok(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    std::str::from_utf8(&bytes)
                        .map(|text| Some(Zeroizing::new(text.to_owned())))
                        .map_err(|_| malformed(service))
                }
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(error) => Err(failure("read", &error)),
            }
        }

        fn write(&self, target: &str, service: &str, text: &str) -> Result<(), SecretStoreError> {
            if text.len() > MAX_BLOB_BYTES {
                return Err(too_long(text.len()));
            }
            self.named(target, service)?
                .set_secret(text.as_bytes())
                .map_err(|error| failure("write", &error))
        }

        fn remove(&self, target: &str, service: &str) -> Result<(), SecretStoreError> {
            match self.named(target, service)?.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(error) => Err(failure("delete", &error)),
            }
        }
    }

    const APP_CREDENTIALS: &str = "app credentials";
    const TOKENS: &str = "tokens";

    impl ConnectionSecrets for CredentialManager {
        fn app_credentials(
            &self,
            owner: ProfileId,
            network: Network,
        ) -> Result<Option<AppCredentials>, SecretStoreError> {
            self.read(&app_credentials_target(owner, network), APP_CREDENTIALS)?
                .map(|stored| {
                    AppCredentials::decode(network, &stored)
                        .ok_or_else(|| malformed(APP_CREDENTIALS))
                })
                .transpose()
        }

        fn set_app_credentials(
            &self,
            owner: ProfileId,
            network: Network,
            credentials: &AppCredentials,
        ) -> Result<(), SecretStoreError> {
            self.write(
                &app_credentials_target(owner, network),
                APP_CREDENTIALS,
                &credentials.encode(),
            )
        }

        fn delete_app_credentials(
            &self,
            owner: ProfileId,
            network: Network,
        ) -> Result<(), SecretStoreError> {
            self.remove(&app_credentials_target(owner, network), APP_CREDENTIALS)
        }

        fn tokens(
            &self,
            owner: ProfileId,
            account: NetworkAccountId,
        ) -> Result<Option<TokenSet>, SecretStoreError> {
            self.read(&tokens_target(owner, account), TOKENS)?
                .map(|stored| TokenSet::decode(&stored).ok_or_else(|| malformed(TOKENS)))
                .transpose()
        }

        fn set_tokens(
            &self,
            owner: ProfileId,
            account: NetworkAccountId,
            tokens: &TokenSet,
        ) -> Result<(), SecretStoreError> {
            let encoded = tokens
                .encode()
                .map_err(|error| SecretStoreError(Box::new(error)))?;
            self.write(&tokens_target(owner, account), TOKENS, &encoded)
        }

        fn delete_tokens(
            &self,
            owner: ProfileId,
            account: NetworkAccountId,
        ) -> Result<(), SecretStoreError> {
            self.remove(&tokens_target(owner, account), TOKENS)
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

    // Fake values, split so secret scanners do not take them for real ones.
    const GOOGLE_ID: &str = concat!("1234567890-abc123def456", ".apps.googleusercontent.com");

    fn credentials(secret: &str) -> AppCredentials {
        AppCredentials::parse(Network::YouTube, GOOGLE_ID, secret).unwrap()
    }

    fn tokens(access: &str, refresh: Option<&str>) -> TokenSet {
        TokenSet::granted(
            &bardo_domain::TokenGrant {
                access_token: bardo_domain::SecretText::new(access),
                refresh_token: refresh.map(bardo_domain::SecretText::new),
                expires_in: std::time::Duration::from_secs(3599),
                scopes: Vec::new(),
            },
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000),
        )
    }

    /// The contract every store keeps for connections. Run against each
    /// adapter.
    fn connection_contract(store: &dyn ConnectionSecrets) {
        let owner = ProfileId::new();
        let other = ProfileId::new();
        let account = NetworkAccountId::new();

        assert_eq!(
            store.app_credentials(owner, Network::YouTube).unwrap(),
            None
        );
        store
            .set_app_credentials(
                owner,
                Network::YouTube,
                &credentials(concat!("GOCSPX", "-secret-0001")),
            )
            .unwrap();
        assert_eq!(
            store.app_credentials(owner, Network::YouTube).unwrap(),
            Some(credentials(concat!("GOCSPX", "-secret-0001")))
        );
        assert_eq!(
            store.app_credentials(other, Network::YouTube).unwrap(),
            None
        );
        store
            .set_app_credentials(
                owner,
                Network::YouTube,
                &credentials(concat!("GOCSPX", "-secret-0002")),
            )
            .unwrap();
        assert_eq!(
            store.app_credentials(owner, Network::YouTube).unwrap(),
            Some(credentials(concat!("GOCSPX", "-secret-0002")))
        );

        assert_eq!(store.tokens(owner, account).unwrap(), None);
        let first = tokens("ya29.access-0001", Some("1//refresh-0001"));
        store.set_tokens(owner, account, &first).unwrap();
        assert_eq!(store.tokens(owner, account).unwrap(), Some(first));
        assert_eq!(store.tokens(other, account).unwrap(), None);
        assert_eq!(store.tokens(owner, NetworkAccountId::new()).unwrap(), None);

        // The largest set that fits is kept whole; one byte more is refused
        // and the earlier tokens stay.
        let room = TokenSet::MAX_STORED_BYTES - "bardo-tokens-1\n1800003599\n\n".len();
        let largest = tokens(&"a".repeat(room), None);
        store.set_tokens(owner, account, &largest).unwrap();
        assert_eq!(store.tokens(owner, account).unwrap(), Some(largest.clone()));
        assert!(
            store
                .set_tokens(owner, account, &tokens(&"a".repeat(room + 1), None))
                .is_err()
        );
        assert_eq!(store.tokens(owner, account).unwrap(), Some(largest));

        store.delete_tokens(owner, account).unwrap();
        store.delete_tokens(owner, account).unwrap();
        assert_eq!(store.tokens(owner, account).unwrap(), None);
        store
            .delete_app_credentials(owner, Network::YouTube)
            .unwrap();
        store
            .delete_app_credentials(owner, Network::YouTube)
            .unwrap();
        assert_eq!(
            store.app_credentials(owner, Network::YouTube).unwrap(),
            None
        );
    }

    #[test]
    fn memory_store_keeps_the_connection_contract() {
        connection_contract(&MemorySecretStore::default());
    }

    #[test]
    fn connection_credentials_are_named_per_profile() {
        let owner = ProfileId::new();
        let account = NetworkAccountId::new();
        assert_eq!(
            app_credentials_target(owner, Network::YouTube),
            format!("Bardo/{owner}/app/youtube")
        );
        assert_eq!(
            tokens_target(owner, account),
            format!("Bardo/{owner}/tokens/{account}")
        );
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

    /// Writes real credentials for a throwaway profile and removes them.
    #[cfg(windows)]
    #[test]
    fn credential_manager_keeps_the_connection_contract() {
        connection_contract(&CredentialManager::new().unwrap());
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
