//! BytePlus credentials (#194, ADR-0025): Seedance 2.5 runs on ModelArk
//! with its API key, and the files ModelArk reads are staged in the user's
//! own TOS bucket with an access key pair. All five fields are saved
//! together as the provider's one key in the secret store, joined by
//! `|`, so the settings file, the key state and log masking treat them
//! like any other key.

use crate::{ApiKey, ApiKeyError, Provider};

/// What joins the fields in the stored key. No field can hold it: keys
/// are letters, digits and base64 punctuation, buckets and regions are
/// lowercase letters, digits and hyphens.
pub const BYTEPLUS_SEPARATOR: char = '|';

/// The region whose ModelArk serves Seedance 2.5 outside mainland China.
pub const BYTEPLUS_DEFAULT_REGION: &str = "ap-southeast-1";

/// One of the fields of the BytePlus card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BytePlusField {
    ApiKey,
    AccessKeyId,
    SecretAccessKey,
    Bucket,
    Region,
}

impl BytePlusField {
    pub const ALL: [BytePlusField; 5] = [
        BytePlusField::ApiKey,
        BytePlusField::AccessKeyId,
        BytePlusField::SecretAccessKey,
        BytePlusField::Bucket,
        BytePlusField::Region,
    ];

    /// Stable name, for the interface's texts.
    pub fn code(self) -> &'static str {
        match self {
            BytePlusField::ApiKey => "api_key",
            BytePlusField::AccessKeyId => "access_key_id",
            BytePlusField::SecretAccessKey => "secret_access_key",
            BytePlusField::Bucket => "bucket",
            BytePlusField::Region => "region",
        }
    }

    /// Whether the field is a secret the logs must never show.
    pub fn is_secret(self) -> bool {
        matches!(
            self,
            BytePlusField::ApiKey | BytePlusField::AccessKeyId | BytePlusField::SecretAccessKey
        )
    }
}

/// Why a field of the BytePlus card is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BytePlusFieldError {
    Required(BytePlusField),
    /// Spaces, `|`, characters keys never have, a bucket name TOS does not
    /// allow (3 to 63 lowercase letters, digits and hyphens, not starting
    /// or ending with one) or a region that is not one.
    Invalid(BytePlusField),
}

impl BytePlusFieldError {
    pub fn field(self) -> BytePlusField {
        match self {
            BytePlusFieldError::Required(field) | BytePlusFieldError::Invalid(field) => field,
        }
    }

    pub fn all() -> impl Iterator<Item = BytePlusFieldError> {
        BytePlusField::ALL.into_iter().flat_map(|field| {
            [
                BytePlusFieldError::Required(field),
                BytePlusFieldError::Invalid(field),
            ]
        })
    }
}

/// The parts of a saved BytePlus key, borrowed from it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BytePlusCredentials<'a> {
    /// Sent to ModelArk as `Authorization: Bearer`.
    pub api_key: &'a str,
    /// The TOS access key pair that signs the staged files' links.
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub bucket: &'a str,
    pub region: &'a str,
}

impl std::fmt::Debug for BytePlusCredentials<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BytePlusCredentials")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

/// Whether `text` is a key part: printable ASCII without spaces or `|`.
fn key_text(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_graphic() && c != BYTEPLUS_SEPARATOR)
}

/// TOS bucket names: 3 to 63 lowercase letters, digits and hyphens, not
/// starting or ending with a hyphen.
pub fn is_bucket_name(text: &str) -> bool {
    (3..=63).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !text.starts_with('-')
        && !text.ends_with('-')
}

/// Region names such as `ap-southeast-1`.
fn is_region(text: &str) -> bool {
    (2..=32).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !text.starts_with('-')
        && !text.ends_with('-')
}

impl<'a> BytePlusCredentials<'a> {
    /// The parts of `key`, when it is a BytePlus key.
    pub fn of(key: &'a ApiKey) -> Option<Self> {
        Self::split(key.expose())
    }

    fn split(text: &'a str) -> Option<Self> {
        let mut parts = text.split(BYTEPLUS_SEPARATOR);
        let found = BytePlusCredentials {
            api_key: parts.next()?,
            access_key_id: parts.next()?,
            secret_access_key: parts.next()?,
            bucket: parts.next()?,
            region: parts.next()?,
        };
        if parts.next().is_some() {
            return None;
        }
        let keys = [found.api_key, found.access_key_id, found.secret_access_key];
        (keys.iter().all(|part| key_text(part))
            && is_bucket_name(found.bucket)
            && is_region(found.region))
        .then_some(found)
    }

    /// Checks the card's fields as the user typed them and joins them into
    /// the key the secret store keeps. Surrounding whitespace is dropped;
    /// an empty region is the default one.
    pub fn compose(
        api_key: &str,
        access_key_id: &str,
        secret_access_key: &str,
        bucket: &str,
        region: &str,
    ) -> Result<ApiKey, Vec<BytePlusFieldError>> {
        // TOS's docs also name regions as their host part (`tos-…`).
        let region = match region.trim() {
            "" => BYTEPLUS_DEFAULT_REGION,
            region => region.strip_prefix("tos-").unwrap_or(region),
        };
        let fields = [
            (BytePlusField::ApiKey, api_key.trim()),
            (BytePlusField::AccessKeyId, access_key_id.trim()),
            (BytePlusField::SecretAccessKey, secret_access_key.trim()),
            (BytePlusField::Bucket, bucket.trim()),
            (BytePlusField::Region, region),
        ];
        let errors: Vec<BytePlusFieldError> = fields
            .iter()
            .filter_map(|&(field, text)| {
                let fits = match field {
                    BytePlusField::Bucket => is_bucket_name(text),
                    BytePlusField::Region => is_region(text),
                    // The redactor masks only parts this long.
                    _ => key_text(text) && text.len() >= ApiKey::MIN_CHARS,
                };
                if text.is_empty() {
                    Some(BytePlusFieldError::Required(field))
                } else if !fits {
                    Some(BytePlusFieldError::Invalid(field))
                } else {
                    None
                }
            })
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        let joined = fields
            .map(|(_, text)| text)
            .join(&BYTEPLUS_SEPARATOR.to_string());
        ApiKey::parse(Provider::BytePlus, &joined).map_err(|_| {
            // Only the whole being too long is left.
            vec![BytePlusFieldError::Invalid(BytePlusField::SecretAccessKey)]
        })
    }
}

/// Whether `key` (already trimmed and checked as printable ASCII) is a
/// BytePlus key.
pub(crate) fn check_key(key: &str) -> Result<(), ApiKeyError> {
    BytePlusCredentials::split(key)
        .map(|_| ())
        .ok_or(ApiKeyError::NotBytePlusCredentials)
}

/// The secret parts of a BytePlus key, for the redactor.
pub(crate) fn secret_parts(key: &str) -> Option<[&str; 3]> {
    BytePlusCredentials::split(key)
        .map(|found| [found.api_key, found.access_key_id, found.secret_access_key])
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = "0b6c2f6e-5a1d-4f3e-9d2a-7c8b9e0f1a2b";
    const AK: &str = "AKLTNjA0ZTVkMmY0YmQ2";
    const SK: &str = "TVRWa1pUVTBNRGM9Pz8+/abc=";

    #[test]
    fn the_five_fields_make_one_key_and_come_back_apart() {
        let key =
            BytePlusCredentials::compose(&format!(" {API}\n"), AK, SK, "bardo-takes", "").unwrap();
        let found = BytePlusCredentials::of(&key).unwrap();
        assert_eq!(found.api_key, API);
        assert_eq!(found.access_key_id, AK);
        assert_eq!(found.secret_access_key, SK);
        assert_eq!(found.bucket, "bardo-takes");
        assert_eq!(found.region, BYTEPLUS_DEFAULT_REGION);
        assert!(!format!("{found:?}").contains(SK));
    }

    #[test]
    fn each_field_says_what_is_wrong_with_it() {
        let errors =
            BytePlusCredentials::compose("", "has space", SK, "Bad_Bucket", "ap|x").unwrap_err();
        assert_eq!(
            errors,
            [
                BytePlusFieldError::Required(BytePlusField::ApiKey),
                BytePlusFieldError::Invalid(BytePlusField::AccessKeyId),
                BytePlusFieldError::Invalid(BytePlusField::Bucket),
                BytePlusFieldError::Invalid(BytePlusField::Region),
            ]
        );
        let short =
            BytePlusCredentials::compose(API, AK, "short", "ab", "ap-southeast-1").unwrap_err();
        assert_eq!(
            short,
            [
                BytePlusFieldError::Invalid(BytePlusField::SecretAccessKey),
                BytePlusFieldError::Invalid(BytePlusField::Bucket),
            ]
        );
    }

    #[test]
    fn bucket_names_follow_tos() {
        for good in ["abc", "bardo-takes-01", &"a".repeat(63)] {
            assert!(is_bucket_name(good), "{good}");
        }
        for bad in ["ab", "-abc", "abc-", "ABC", "a_b", "a.b", &"a".repeat(64)] {
            assert!(!is_bucket_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_pasted_key_must_have_all_five_parts() {
        let joined = [API, AK, SK, "bardo-takes", "ap-southeast-1"].join("|");
        assert!(ApiKey::parse(Provider::BytePlus, &joined).is_ok());
        for bad in [
            API.to_owned(),
            [API, AK, SK, "bardo-takes"].join("|"),
            [API, AK, SK, "bardo-takes", "ap-southeast-1", "x"].join("|"),
            [API, "", SK, "bardo-takes", "ap-southeast-1"].join("|"),
        ] {
            assert_eq!(
                ApiKey::parse(Provider::BytePlus, &bad),
                Err(ApiKeyError::NotBytePlusCredentials),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_hint_and_the_masked_parts_come_from_the_secret_fields() {
        let key =
            BytePlusCredentials::compose(API, AK, SK, "bardo-takes", "ap-southeast-1").unwrap();
        assert_eq!(key.hint(), "…1a2b");
        let parts = key.sensitive_parts();
        for part in [API, AK, SK] {
            assert!(parts.contains(&part), "{part}");
        }
        assert!(!parts.contains(&"ap-southeast-1"));
        assert!(!parts.contains(&"bardo-takes"));
    }
}
