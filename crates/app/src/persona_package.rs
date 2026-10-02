//! The persona package (PRD stories 17-19): one persona as a small JSON
//! file the user moves to another machine or profile. It carries what the
//! persona says and how it sounds (name, tone, script style, presets) and
//! a reference to its voice (provider, voice id, voice name). It never
//! carries audio, provider keys or ids from the exporting machine: the
//! voice stays in its provider account, and the importer's own account
//! has to have it before the persona can narrate.
//!
//! The format is versioned. A reader accepts every version up to its own
//! and refuses newer ones with a message to update Bardo, so an older
//! release never half-reads a package it does not understand.
//!
//! ```json
//! {
//!   "format": "bardo-persona",
//!   "version": 1,
//!   "persona": {
//!     "name": "Documentary Narrator (en-US)",
//!     "voice": { "provider": "elevenlabs", "id": "FrS6cKLB1wg4WYgPa9GW", "name": "Wyatt" },
//!     "tone": "Sober, measured and authoritative.",
//!     "script_style": "Clear chronological structure.",
//!     "presets": { "stability": 60, "similarity": 75, "style": 0, "speed": 100 }
//!   }
//! }
//! ```

use bardo_domain::{GenerationPresets, PersonaDetails, PersonaDraft, Provider, VoiceRef};
use serde::{Deserialize, Serialize};

/// Names the format, so another app's JSON is not mistaken for a persona.
pub const FORMAT: &str = "bardo-persona";

/// The newest version this build writes and reads.
pub const VERSION: u64 = 1;

/// The file extension the save and open dialogs suggest.
pub const EXTENSION: &str = "bardo-persona";

/// Packages are a few kilobytes; anything far larger is not one, and is
/// not read into memory.
pub const MAX_BYTES: u64 = 256 * 1024;

/// Why a file cannot be imported as a persona.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    /// Not JSON, not a persona package, or missing required fields.
    #[error("not a Bardo persona package")]
    NotAPackage,
    /// Made by a newer Bardo, in a format this build does not know.
    #[error("persona package version {0} is newer than this Bardo reads ({VERSION})")]
    NewerVersion(u64),
    /// A package of a known version whose persona breaks the persona
    /// rules (for example an edited file with an empty name).
    #[error("the packaged persona is invalid")]
    InvalidPersona,
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    format: String,
    version: u64,
    persona: T,
}

/// Only what is needed to decide whether the rest can be read.
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u64,
}

#[derive(Serialize, Deserialize)]
struct PersonaV1 {
    name: String,
    voice: VoiceV1,
    tone: String,
    script_style: String,
    presets: PresetsV1,
    /// Written only when set, so packages of other voices read as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    realistic_voice: bool,
}

#[derive(Serialize, Deserialize)]
struct VoiceV1 {
    provider: String,
    id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct PresetsV1 {
    stability: u8,
    similarity: u8,
    style: u8,
    speed: u8,
}

/// The package for a persona, as pretty-printed JSON so a user can read
/// what it carries before sharing it.
pub fn encode(details: &PersonaDetails) -> String {
    let voice = details.voice();
    let presets = details.presets();
    let envelope = Envelope {
        format: FORMAT.to_owned(),
        version: VERSION,
        persona: PersonaV1 {
            name: details.name().to_owned(),
            voice: VoiceV1 {
                provider: voice.provider().code().to_owned(),
                id: voice.id().to_owned(),
                name: voice.name().to_owned(),
            },
            tone: details.tone().to_owned(),
            script_style: details.script_style().to_owned(),
            presets: PresetsV1 {
                stability: presets.stability,
                similarity: presets.similarity,
                style: presets.style,
                speed: presets.speed,
            },
            realistic_voice: details.realistic_voice(),
        },
    };
    let mut json = serde_json::to_string_pretty(&envelope).expect("a persona package serializes");
    json.push('\n');
    json
}

/// Reads a package back into valid persona details.
pub fn decode(text: &str) -> Result<PersonaDetails, PackageError> {
    let header: Header = serde_json::from_str(text).map_err(|_| PackageError::NotAPackage)?;
    if header.format != FORMAT || header.version == 0 {
        return Err(PackageError::NotAPackage);
    }
    if header.version > VERSION {
        return Err(PackageError::NewerVersion(header.version));
    }
    let envelope: Envelope<PersonaV1> =
        serde_json::from_str(text).map_err(|_| PackageError::NotAPackage)?;
    let persona = envelope.persona;
    let provider: Provider = persona
        .voice
        .provider
        .parse()
        .map_err(|_| PackageError::InvalidPersona)?;
    let voice = VoiceRef::new(provider, &persona.voice.id, &persona.voice.name)
        .map_err(|_| PackageError::InvalidPersona)?;
    PersonaDetails::validate(PersonaDraft {
        name: persona.name,
        voice: Some(voice),
        tone: persona.tone,
        script_style: persona.script_style,
        presets: GenerationPresets {
            stability: persona.presets.stability,
            similarity: persona.presets.similarity,
            style: persona.presets.style,
            speed: persona.presets.speed,
        },
        realistic_voice: persona.realistic_voice,
    })
    .map_err(|_| PackageError::InvalidPersona)
}

/// A file name for the package: the persona's name without characters
/// Windows refuses in file names, plus the extension.
pub fn file_name(details: &PersonaDetails) -> String {
    let stem: String = details
        .name()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows also refuses names ending in a dot or space, and device
    // names (CON, NUL, COM1...) whatever the extension.
    let stem = stem.trim_end_matches(['.', ' ']);
    let stem = if stem.is_empty() { "persona" } else { stem };
    let device = |name: &str| {
        let name = name.to_ascii_uppercase();
        matches!(name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((name.starts_with("COM") || name.starts_with("LPT"))
                && name.len() == 4
                && name.as_bytes()[3].is_ascii_digit())
    };
    let base = stem.split('.').next().unwrap_or(stem).trim_end();
    if device(base) {
        format!("{stem}_.{EXTENSION}")
    } else {
        format!("{stem}.{EXTENSION}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details() -> PersonaDetails {
        PersonaDetails::validate(PersonaDraft {
            name: "Narradora Documental (pt-BR)".into(),
            voice: Some(VoiceRef::elevenlabs("myClone01", "Minha voz \"clonada\"").unwrap()),
            tone: "Sóbria.\nSem exageros: os fatos falam por si. 🎙️".into(),
            script_style: "Frases curtas.".into(),
            presets: GenerationPresets {
                stability: 0,
                similarity: 100,
                style: 35,
                speed: 70,
            },
            realistic_voice: true,
        })
        .unwrap()
    }

    #[test]
    fn a_package_round_trips_exactly() {
        let original = details();
        assert_eq!(decode(&encode(&original)), Ok(original));
        for persona in bardo_domain::Persona::defaults(bardo_domain::ProfileId::new()) {
            assert_eq!(decode(&encode(&persona.details)), Ok(persona.details));
        }
    }

    #[test]
    fn the_package_is_readable_json_with_a_format_and_version() {
        let json: serde_json::Value = serde_json::from_str(&encode(&details())).unwrap();
        assert_eq!(json["format"], "bardo-persona");
        assert_eq!(json["version"], 1);
        assert_eq!(json["persona"]["voice"]["provider"], "elevenlabs");
        assert_eq!(json["persona"]["voice"]["id"], "myClone01");
        assert_eq!(json["persona"]["presets"]["speed"], 70);
    }

    #[test]
    fn the_package_carries_only_the_persona_fields() {
        let json: serde_json::Value = serde_json::from_str(&encode(&details())).unwrap();
        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(&json), ["format", "persona", "version"]);
        assert_eq!(
            keys(&json["persona"]),
            [
                "name",
                "presets",
                "realistic_voice",
                "script_style",
                "tone",
                "voice"
            ]
        );
        assert_eq!(keys(&json["persona"]["voice"]), ["id", "name", "provider"]);
        assert_eq!(
            keys(&json["persona"]["presets"]),
            ["similarity", "speed", "stability", "style"]
        );
    }

    #[test]
    fn the_realistic_voice_flag_is_written_only_when_set() {
        let stock = &bardo_domain::Persona::defaults(bardo_domain::ProfileId::new())[0].details;
        assert!(!encode(stock).contains("realistic_voice"));
        // A package written before the flag existed reads as unflagged.
        let mut older: serde_json::Value = serde_json::from_str(&encode(&details())).unwrap();
        older["persona"]
            .as_object_mut()
            .unwrap()
            .remove("realistic_voice");
        let older = older.to_string();
        assert!(!older.contains("realistic_voice"), "{older}");
        assert_eq!(decode(&older).map(|d| d.realistic_voice()), Ok(false));
    }

    #[test]
    fn a_newer_version_is_refused_with_its_number() {
        let newer = encode(&details()).replace("\"version\": 1", "\"version\": 2");
        assert_eq!(decode(&newer), Err(PackageError::NewerVersion(2)));
        // Even when the rest is something this build cannot read at all.
        let alien = r#"{"format": "bardo-persona", "version": 7, "persona": {"voices": []}}"#;
        assert_eq!(decode(alien), Err(PackageError::NewerVersion(7)));
        let far = r#"{"format": "bardo-persona", "version": 4294967296}"#;
        assert_eq!(decode(far), Err(PackageError::NewerVersion(4_294_967_296)));
        assert!(
            PackageError::NewerVersion(7).to_string().contains('7'),
            "the message names the version"
        );
    }

    #[test]
    fn other_files_are_not_packages() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"format": "something-else", "version": 1, "persona": {}}"#,
            r#"{"format": "bardo-persona", "version": 0}"#,
            r#"{"format": "bardo-persona", "version": -1}"#,
            r#"{"format": "bardo-persona", "version": 1}"#,
            r#"{"format": "bardo-persona", "version": 1, "persona": {"name": "x"}}"#,
        ] {
            assert_eq!(decode(text), Err(PackageError::NotAPackage), "{text}");
        }
    }

    #[test]
    fn an_edited_package_cannot_smuggle_in_an_invalid_persona() {
        let package = encode(&details());
        for (from, to) in [
            (
                "\"name\": \"Narradora Documental (pt-BR)\"",
                "\"name\": \"  \"",
            ),
            ("\"id\": \"myClone01\"", "\"id\": \"not an id\""),
            ("\"provider\": \"elevenlabs\"", "\"provider\": \"claude\""),
            ("\"provider\": \"elevenlabs\"", "\"provider\": \"nobody\""),
            ("\"speed\": 70", "\"speed\": 130"),
        ] {
            assert!(package.contains(from), "{from}");
            let edited = package.replace(from, to);
            assert_eq!(decode(&edited), Err(PackageError::InvalidPersona), "{to}");
        }
        // Out of a preset's integer type entirely: not this format.
        let edited = package.replace("\"speed\": 70", "\"speed\": 300");
        assert_eq!(decode(&edited), Err(PackageError::NotAPackage));
    }

    #[test]
    fn unknown_fields_of_a_known_version_are_ignored() {
        let package = encode(&details()).replacen('{', "{\n  \"exported_by\": \"Bardo\",", 1);
        assert_eq!(decode(&package), Ok(details()));
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        let named = |name: &str| {
            let details = PersonaDetails::validate(PersonaDraft {
                name: name.into(),
                voice: Some(VoiceRef::elevenlabs("abc", "Wyatt").unwrap()),
                ..PersonaDraft::default()
            })
            .unwrap();
            file_name(&details)
        };
        assert_eq!(
            named("Narradora Documental (pt-BR)"),
            "Narradora Documental (pt-BR).bardo-persona"
        );
        assert_eq!(
            named("A/B: \"C\" <d>?*|\\"),
            "A_B_ _C_ _d_____.bardo-persona"
        );
        assert_eq!(named("Ends with a dot."), "Ends with a dot.bardo-persona");
        assert_eq!(named("..."), "persona.bardo-persona");
        assert_eq!(named("Con"), "Con_.bardo-persona");
        assert_eq!(named("com1"), "com1_.bardo-persona");
        assert_eq!(named("Nul.txt"), "Nul.txt_.bardo-persona");
        assert_eq!(named("Console"), "Console.bardo-persona");
    }
}
