//! The settings file (issue #162, ADR-0014): the settings the user picked,
//! sealed under a password, to carry them into another copy of Bardo (the
//! one built from source, the installed one, another computer).
//!
//! A small JSON envelope says what the file is and how it was sealed; the
//! contents (`settings_transfer`) are sealed with XChaCha20-Poly1305 under
//! a key derived from the password with Argon2id. The envelope is the
//! sealing's associated data, so an edit to it fails the same way a wrong
//! password does. The password is never stored: a lost one cannot be
//! recovered.
//!
//! ```json
//! {
//!   "format": "bardo-settings",
//!   "version": 1,
//!   "bardo_version": "0.1.1",
//!   "created_at": 1792224000,
//!   "kdf": { "algorithm": "argon2id", "memory_kib": 65536, "iterations": 3,
//!            "parallelism": 1, "salt": "…" },
//!   "cipher": { "algorithm": "xchacha20poly1305", "nonce": "…" },
//!   "sealed": "…"
//! }
//! ```
//!
//! The envelope's version changes only with how the file is sealed. What
//! the contents hold grows without it: a reader skips what it does not
//! know (`settings_transfer`).

use std::time::{Duration, SystemTime};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Names the format, so another app's JSON is not mistaken for settings.
pub const FORMAT: &str = "bardo-settings";

/// The newest envelope this build writes and reads.
pub const VERSION: u64 = 1;

/// The file extension the save and open dialogs suggest.
pub const EXTENSION: &str = "bardo-settings";

/// Settings take a few kilobytes; anything far larger is not a settings
/// file, and is not read into memory.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// The shortest password a settings file is sealed with.
pub const MIN_PASSWORD_CHARS: usize = 10;

const KDF: &str = "argon2id";
const CIPHER: &str = "xchacha20poly1305";
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const KEY_BYTES: usize = 32;

/// How hard the password is to guess at: 64 MiB and three passes, about
/// half a second on a laptop. Above OWASP's minimum for Argon2id, and run
/// once per export or import, off the UI thread.
const SEAL_COST: Cost = Cost {
    memory_kib: 64 * 1024,
    iterations: 3,
    parallelism: 1,
};

/// The most a file may ask for, so a crafted one cannot make Bardo
/// allocate gigabytes or spin for minutes before the password is checked.
const MAX_MEMORY_KIB: u32 = 1024 * 1024;
const MAX_ITERATIONS: u32 = 16;
const MAX_PARALLELISM: u32 = 8;

/// The password a settings file is sealed and opened with. Wiped from
/// memory when dropped; `Debug` hides it.
#[derive(Clone)]
pub struct SettingsPassword(Zeroizing<String>);

impl SettingsPassword {
    /// A password for a new file: long enough, and typed the same twice.
    pub fn choose(password: &str, again: &str) -> Result<Self, PasswordError> {
        if password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(PasswordError::TooShort);
        }
        if password != again {
            return Err(PasswordError::DoNotMatch);
        }
        Ok(Self(Zeroizing::new(password.to_owned())))
    }

    /// A password as typed to open a file; a wrong one fails `open`.
    pub fn typed(password: &str) -> Self {
        Self(Zeroizing::new(password.to_owned()))
    }

    fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Debug for SettingsPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettingsPassword(<redacted>)")
    }
}

/// Why a password cannot seal a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("the password is shorter than {MIN_PASSWORD_CHARS} characters")]
    TooShort,
    #[error("the two passwords differ")]
    DoNotMatch,
}

/// Why a file cannot be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingsFileError {
    /// Not JSON, not a settings file, or a part missing.
    #[error("not a Bardo settings file")]
    NotASettingsFile,
    /// Sealed by a newer Bardo, in a way this build does not know.
    #[error("settings file version {0} is newer than this Bardo reads ({VERSION})")]
    NewerVersion(u64),
    /// The password is wrong, or the file was changed or damaged: the
    /// sealing cannot tell the two apart, and neither does the message.
    #[error("wrong password, or the file is damaged")]
    WrongPasswordOrDamaged,
    /// The system gave no random bytes to seal with.
    #[error("no randomness to seal the file with")]
    NoRandomness,
}

/// What the envelope says about a file, readable without the password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFileInfo {
    /// The Bardo that made it, e.g. `0.1.1`.
    pub bardo_version: String,
    pub created_at: SystemTime,
}

/// A file opened with its password.
pub struct OpenedSettings {
    pub info: SettingsFileInfo,
    /// The contents, wiped from memory when dropped.
    pub contents: Zeroizing<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cost {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

/// The part of the file the sealing authenticates.
#[derive(Serialize, Deserialize)]
struct Header {
    format: String,
    version: u64,
    bardo_version: String,
    created_at: u64,
    kdf: Kdf,
    cipher: Cipher,
}

#[derive(Serialize, Deserialize)]
struct Kdf {
    algorithm: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct Cipher {
    algorithm: String,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    #[serde(flatten)]
    header: Header,
    sealed: String,
}

/// Only what is needed to decide whether the rest can be read.
#[derive(Deserialize)]
struct Probe {
    format: String,
    version: u64,
}

/// Seals `contents` under `password`, as the bytes of a settings file.
pub fn seal(
    contents: &[u8],
    password: &SettingsPassword,
    created_at: SystemTime,
) -> Result<Vec<u8>, SettingsFileError> {
    seal_with(contents, password, created_at, SEAL_COST)
}

fn seal_with(
    contents: &[u8],
    password: &SettingsPassword,
    created_at: SystemTime,
    cost: Cost,
) -> Result<Vec<u8>, SettingsFileError> {
    let mut salt = [0u8; SALT_BYTES];
    let mut nonce = [0u8; NONCE_BYTES];
    getrandom::fill(&mut salt).map_err(|_| SettingsFileError::NoRandomness)?;
    getrandom::fill(&mut nonce).map_err(|_| SettingsFileError::NoRandomness)?;
    let header = Header {
        format: FORMAT.to_owned(),
        version: VERSION,
        bardo_version: env!("CARGO_PKG_VERSION").to_owned(),
        created_at: created_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        kdf: Kdf {
            algorithm: KDF.to_owned(),
            memory_kib: cost.memory_kib,
            iterations: cost.iterations,
            parallelism: cost.parallelism,
            salt: BASE64.encode(salt),
        },
        cipher: Cipher {
            algorithm: CIPHER.to_owned(),
            nonce: BASE64.encode(nonce),
        },
    };
    let key = derive_key(password, &salt, cost)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice())
        .map_err(|_| SettingsFileError::WrongPasswordOrDamaged)?;
    let aad = associated_data(&header);
    let sealed = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: contents,
                aad: &aad,
            },
        )
        .map_err(|_| SettingsFileError::WrongPasswordOrDamaged)?;
    let file = File {
        header,
        sealed: BASE64.encode(sealed),
    };
    let mut json = serde_json::to_vec_pretty(&file).expect("a settings file serializes");
    json.push(b'\n');
    Ok(json)
}

/// What the file is, without the password: refuses what is not a settings
/// file, or one sealed by a newer Bardo.
pub fn inspect(bytes: &[u8]) -> Result<SettingsFileInfo, SettingsFileError> {
    read(bytes).map(|file| info(&file.header))
}

/// Opens a settings file with its password.
pub fn open(
    bytes: &[u8],
    password: &SettingsPassword,
) -> Result<OpenedSettings, SettingsFileError> {
    let file = read(bytes)?;
    let header = &file.header;
    let damaged = SettingsFileError::WrongPasswordOrDamaged;
    if header.kdf.algorithm != KDF || header.cipher.algorithm != CIPHER {
        return Err(SettingsFileError::NotASettingsFile);
    }
    let cost = Cost {
        memory_kib: header.kdf.memory_kib,
        iterations: header.kdf.iterations,
        parallelism: header.kdf.parallelism,
    };
    if cost.memory_kib > MAX_MEMORY_KIB
        || cost.iterations > MAX_ITERATIONS
        || cost.parallelism > MAX_PARALLELISM
    {
        return Err(damaged);
    }
    let salt = BASE64.decode(&header.kdf.salt).map_err(|_| damaged)?;
    let nonce: [u8; NONCE_BYTES] = BASE64
        .decode(&header.cipher.nonce)
        .ok()
        .and_then(|nonce| nonce.try_into().ok())
        .ok_or(damaged)?;
    let sealed = BASE64.decode(&file.sealed).map_err(|_| damaged)?;
    let key = derive_key(password, &salt, cost)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| damaged)?;
    let aad = associated_data(header);
    let contents = cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &sealed,
                aad: &aad,
            },
        )
        .map_err(|_| damaged)?;
    Ok(OpenedSettings {
        info: info(header),
        contents: Zeroizing::new(contents),
    })
}

fn read(bytes: &[u8]) -> Result<File, SettingsFileError> {
    let not = SettingsFileError::NotASettingsFile;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(not);
    }
    let probe: Probe = serde_json::from_slice(bytes).map_err(|_| not)?;
    if probe.format != FORMAT || probe.version == 0 {
        return Err(not);
    }
    if probe.version > VERSION {
        return Err(SettingsFileError::NewerVersion(probe.version));
    }
    serde_json::from_slice(bytes).map_err(|_| not)
}

fn info(header: &Header) -> SettingsFileInfo {
    SettingsFileInfo {
        bardo_version: header.bardo_version.clone(),
        created_at: SystemTime::UNIX_EPOCH + Duration::from_secs(header.created_at),
    }
}

/// The envelope as the sealing authenticates it: its fields, in this
/// order, compact. A file whose envelope was edited reads back different.
fn associated_data(header: &Header) -> Vec<u8> {
    serde_json::to_vec(header).expect("a settings header serializes")
}

fn derive_key(
    password: &SettingsPassword,
    salt: &[u8],
    cost: Cost,
) -> Result<Zeroizing<[u8; KEY_BYTES]>, SettingsFileError> {
    let damaged = SettingsFileError::WrongPasswordOrDamaged;
    let params = Params::new(
        cost.memory_kib,
        cost.iterations,
        cost.parallelism,
        Some(KEY_BYTES),
    )
    .map_err(|_| damaged)?;
    let mut key = Zeroizing::new([0u8; KEY_BYTES]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.bytes(), salt, key.as_mut_slice())
        .map_err(|_| damaged)?;
    Ok(key)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Cheap enough for tests in a debug build; the real cost is checked
    /// by its own test.
    const TEST_COST: Cost = Cost {
        memory_kib: 64,
        iterations: 1,
        parallelism: 1,
    };

    pub(crate) fn seal_cheaply(contents: &[u8], password: &SettingsPassword) -> Vec<u8> {
        seal_with(contents, password, SystemTime::UNIX_EPOCH, TEST_COST).unwrap()
    }

    fn password() -> SettingsPassword {
        SettingsPassword::choose("correct horse", "correct horse").unwrap()
    }

    #[test]
    fn a_sealed_file_opens_with_its_password() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_224_000);
        let bytes = seal_with(b"the contents", &password(), at, TEST_COST).unwrap();
        let opened = open(&bytes, &password()).unwrap();
        assert_eq!(opened.contents.as_slice(), b"the contents");
        assert_eq!(opened.info.created_at, at);
        assert_eq!(opened.info.bardo_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(inspect(&bytes).unwrap(), opened.info);
    }

    #[test]
    fn the_contents_never_appear_in_the_file() {
        let secret = "sk-ant-api03-very-secret";
        let bytes = seal_cheaply(secret.as_bytes(), &password());
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains(secret));
        assert!(!text.contains("correct horse"));
    }

    #[test]
    fn each_file_has_its_own_salt_and_nonce() {
        let a = seal_cheaply(b"same", &password());
        let b = seal_cheaply(b"same", &password());
        assert_ne!(a, b);
    }

    #[test]
    fn a_wrong_password_and_a_changed_byte_fail_the_same_way() {
        let bytes = seal_cheaply(b"the contents", &password());
        assert_eq!(
            open(&bytes, &SettingsPassword::typed("wrong horse!")).err(),
            Some(SettingsFileError::WrongPasswordOrDamaged)
        );
        let mut file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let sealed = file["sealed"].as_str().unwrap();
        let mut raw = BASE64.decode(sealed).unwrap();
        raw[0] ^= 1;
        file["sealed"] = BASE64.encode(raw).into();
        let changed = serde_json::to_vec(&file).unwrap();
        assert_eq!(
            open(&changed, &password()).err(),
            Some(SettingsFileError::WrongPasswordOrDamaged)
        );
    }

    #[test]
    fn an_edited_envelope_fails_like_a_wrong_password() {
        let bytes = seal_cheaply(b"the contents", &password());
        let mut file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        file["bardo_version"] = "9.9.9".into();
        let edited = serde_json::to_vec(&file).unwrap();
        assert_eq!(
            open(&edited, &password()).err(),
            Some(SettingsFileError::WrongPasswordOrDamaged)
        );
    }

    #[test]
    fn a_file_asking_for_too_much_work_is_refused_before_the_password() {
        let bytes = seal_cheaply(b"the contents", &password());
        let mut file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        file["kdf"]["memory_kib"] = (MAX_MEMORY_KIB + 1).into();
        let greedy = serde_json::to_vec(&file).unwrap();
        assert_eq!(
            open(&greedy, &password()).err(),
            Some(SettingsFileError::WrongPasswordOrDamaged)
        );
    }

    #[test]
    fn other_files_and_newer_versions_are_refused() {
        for bytes in [
            &b"not json"[..],
            br#"{"format":"bardo-persona","version":1}"#,
            br#"{"format":"bardo-settings","version":0}"#,
            br#"{"format":"bardo-settings","version":1}"#,
        ] {
            assert_eq!(
                inspect(bytes).err(),
                Some(SettingsFileError::NotASettingsFile)
            );
        }
        assert_eq!(
            inspect(br#"{"format":"bardo-settings","version":2}"#).err(),
            Some(SettingsFileError::NewerVersion(2))
        );
    }

    #[test]
    fn passwords_must_be_long_enough_and_typed_twice() {
        assert_eq!(
            SettingsPassword::choose("short", "short").err(),
            Some(PasswordError::TooShort)
        );
        assert_eq!(
            SettingsPassword::choose("long enough!", "long enough?").err(),
            Some(PasswordError::DoNotMatch)
        );
        // Characters, not bytes: ten accented letters are long enough.
        assert!(SettingsPassword::choose("ãããããããããã", "ãããããããããã").is_ok());
        assert_eq!(format!("{:?}", password()), "SettingsPassword(<redacted>)");
    }

    #[test]
    fn real_files_are_sealed_at_the_documented_cost() {
        let bytes = seal(b"x", &password(), SystemTime::UNIX_EPOCH).unwrap();
        let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(file["kdf"]["algorithm"], "argon2id");
        assert_eq!(file["kdf"]["memory_kib"], 65536);
        assert_eq!(file["kdf"]["iterations"], 3);
        assert_eq!(file["cipher"]["algorithm"], "xchacha20poly1305");
        assert_eq!(open(&bytes, &password()).unwrap().contents.as_slice(), b"x");
    }
}
