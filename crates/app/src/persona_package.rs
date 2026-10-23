//! The persona package (PRD stories 17-19): one persona as a JSON file the
//! user moves to another machine or profile. It carries what the persona
//! says and how it sounds (name, tone, script style, presets) and a
//! reference to its voice (provider, voice id, voice name). It never
//! carries audio, provider keys or ids from the exporting machine: the
//! voice stays in its provider account, and the importer's own account
//! has to have it before the persona can narrate.
//!
//! Version 2 (#188) adds the persona's look when it is a drawing: the image
//! as base64, the description and prompt it was drawn from, and whether it
//! is realistic. A photo of the user is someone's likeness and never goes,
//! nor does any provider's asset id. A persona with no drawn look is still
//! written as version 1, so older releases import it.
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

use bardo_domain::{
    GenerationPresets, LookImage, MAX_LOOK_PROMPT, PersonaDetails, PersonaDraft, PersonaLook,
    Provider, VoiceRef, look_description,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Names the format, so another app's JSON is not mistaken for a persona.
pub const FORMAT: &str = "bardo-persona";

/// The newest version this build writes and reads.
pub const VERSION: u64 = 2;

/// The version of a package without a look, which every release reads.
const WITHOUT_LOOK: u64 = 1;

/// The file extension the save and open dialogs suggest.
pub const EXTENSION: &str = "bardo-persona";

/// A package is a few kilobytes, or up to a look's 10 MB as base64 with a
/// look; anything larger is not one, and is not read into memory.
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;

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
    /// A drawn look, from version 2 on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    look: Option<LookV2>,
}

#[derive(Serialize, Deserialize)]
struct LookV2 {
    /// The image file as base64.
    image: String,
    /// The image's type, for people reading the file: `image/png` or
    /// `image/jpeg`. The image itself says which.
    media_type: String,
    description: String,
    prompt: String,
    realistic: bool,
}

/// A drawn look as a package carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedLook {
    pub image: LookImage,
    pub description: String,
    pub prompt: String,
    pub realistic: bool,
}

/// What a package holds: the persona, and its look when it is a drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub details: PersonaDetails,
    pub look: Option<PackagedLook>,
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
/// what it carries before sharing it. `look` goes only when it is a
/// drawing; the package is version 1 without one.
pub fn encode(details: &PersonaDetails, look: Option<&PersonaLook>) -> String {
    let voice = details.voice();
    let presets = details.presets();
    let look = look.filter(|look| look.is_shareable()).map(|look| {
        let drawing = look.drawing.clone().unwrap_or_default();
        LookV2 {
            image: base64::engine::general_purpose::STANDARD.encode(look.image.bytes()),
            media_type: look.image.format().mime().to_owned(),
            description: drawing.description,
            prompt: drawing.prompt,
            realistic: look.realistic,
        }
    });
    let envelope = Envelope {
        format: FORMAT.to_owned(),
        version: if look.is_some() {
            VERSION
        } else {
            WITHOUT_LOOK
        },
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
            look,
        },
    };
    let mut json = serde_json::to_string_pretty(&envelope).expect("a persona package serializes");
    json.push('\n');
    json
}

/// Reads a package back into valid persona details, and its look.
pub fn decode(text: &str) -> Result<Package, PackageError> {
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
    // Version 1 has no look; one written there anyway is not read.
    let look = match persona.look.filter(|_| header.version >= 2) {
        None => None,
        Some(look) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(look.image.as_bytes())
                .map_err(|_| PackageError::InvalidPersona)?;
            let image = LookImage::check(bytes).map_err(|_| PackageError::InvalidPersona)?;
            let description =
                look_description(&look.description).map_err(|_| PackageError::InvalidPersona)?;
            let prompt = look.prompt.trim();
            if prompt.is_empty() || prompt.chars().count() > MAX_LOOK_PROMPT {
                return Err(PackageError::InvalidPersona);
            }
            Some(PackagedLook {
                image,
                description,
                prompt: prompt.to_owned(),
                realistic: look.realistic,
            })
        }
    };
    let provider: Provider = persona
        .voice
        .provider
        .parse()
        .map_err(|_| PackageError::InvalidPersona)?;
    let voice = VoiceRef::new(provider, &persona.voice.id, &persona.voice.name)
        .map_err(|_| PackageError::InvalidPersona)?;
    let details = PersonaDetails::validate(PersonaDraft {
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
    .map_err(|_| PackageError::InvalidPersona)?;
    Ok(Package { details, look })
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

    /// The details alone, from a package without a look.
    fn decode_details(text: &str) -> Result<PersonaDetails, PackageError> {
        decode(text).map(|package| {
            assert_eq!(package.look, None);
            package.details
        })
    }

    fn encode_details(details: &PersonaDetails) -> String {
        encode(details, None)
    }

    const LOOK: &[u8] = include_bytes!("../../ai/tests/fixtures/gemini/look-2x3.png");

    fn drawn_look(realistic: bool) -> PersonaLook {
        PersonaLook::drawn(
            bardo_domain::PersonaId::new(),
            LookImage::check(LOOK.to_vec()).unwrap(),
            bardo_domain::LookDrawing {
                description: "Uma faroleira de cabelo grisalho".into(),
                prompt: "Waist-up portrait of an original lighthouse keeper".into(),
                template: None,
                writer_model: "claude-test".into(),
                image_model: "gemini-test".into(),
                job: Some(bardo_domain::JobId::new()),
            },
            realistic,
            std::time::SystemTime::UNIX_EPOCH,
        )
    }

    #[test]
    fn a_package_round_trips_exactly() {
        let original = details();
        assert_eq!(decode_details(&encode_details(&original)), Ok(original));
        for persona in bardo_domain::Persona::defaults(bardo_domain::ProfileId::new()) {
            assert_eq!(
                decode_details(&encode_details(&persona.details)),
                Ok(persona.details)
            );
        }
    }

    #[test]
    fn a_drawn_look_travels_in_version_2() {
        let look = drawn_look(true);
        let package = encode(&details(), Some(&look));
        let json: serde_json::Value = serde_json::from_str(&package).unwrap();
        assert_eq!(json["version"], 2);
        assert_eq!(json["persona"]["look"]["media_type"], "image/png");
        let read = decode(&package).unwrap();
        assert_eq!(read.details, details());
        assert_eq!(
            read.look,
            Some(PackagedLook {
                image: look.image.clone(),
                description: "Uma faroleira de cabelo grisalho".into(),
                prompt: "Waist-up portrait of an original lighthouse keeper".into(),
                realistic: true,
            })
        );
        // Only what the look is: no ids, models or jobs from this machine.
        let mut keys: Vec<_> = json["persona"]["look"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            ["description", "image", "media_type", "prompt", "realistic"]
        );
    }

    #[test]
    fn a_photo_of_the_user_never_leaves() {
        let mut photo = drawn_look(true);
        photo.source = bardo_domain::LookSource::Imported;
        photo.drawing = None;
        let package = encode(&details(), Some(&photo));
        assert!(!package.contains("\"look\""), "{package}");
        assert!(package.contains("\"version\": 1"), "older Bardo reads it");
        assert_eq!(decode(&package).unwrap().look, None);
    }

    #[test]
    fn an_edited_look_is_refused() {
        let package = encode(&details(), Some(&drawn_look(false)));
        let mut json: serde_json::Value = serde_json::from_str(&package).unwrap();
        for image in ["not base64!", "R0lGODlh"] {
            json["persona"]["look"]["image"] = image.into();
            assert_eq!(
                decode(&json.to_string()),
                Err(PackageError::InvalidPersona),
                "{image}"
            );
        }
        let words = [
            ("description", "   ".to_owned()),
            ("description", "x".repeat(1_001)),
            ("prompt", String::new()),
            ("prompt", "x".repeat(bardo_domain::MAX_LOOK_PROMPT + 1)),
        ];
        for (field, text) in words {
            let mut json: serde_json::Value = serde_json::from_str(&package).unwrap();
            json["persona"]["look"][field] = text.clone().into();
            assert_eq!(
                decode(&json.to_string()),
                Err(PackageError::InvalidPersona),
                "{field}: {} characters",
                text.len()
            );
        }
        // A look in a version 1 package is not one this format has.
        let mut older: serde_json::Value = serde_json::from_str(&package).unwrap();
        older["version"] = 1.into();
        assert_eq!(decode(&older.to_string()).unwrap().look, None);
    }

    #[test]
    fn the_package_is_readable_json_with_a_format_and_version() {
        let json: serde_json::Value = serde_json::from_str(&encode_details(&details())).unwrap();
        assert_eq!(json["format"], "bardo-persona");
        assert_eq!(json["version"], 1);
        assert_eq!(json["persona"]["voice"]["provider"], "elevenlabs");
        assert_eq!(json["persona"]["voice"]["id"], "myClone01");
        assert_eq!(json["persona"]["presets"]["speed"], 70);
    }

    #[test]
    fn the_package_carries_only_the_persona_fields() {
        let json: serde_json::Value = serde_json::from_str(&encode_details(&details())).unwrap();
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
        assert!(!encode_details(stock).contains("realistic_voice"));
        // A package written before the flag existed reads as unflagged.
        let mut older: serde_json::Value =
            serde_json::from_str(&encode_details(&details())).unwrap();
        older["persona"]
            .as_object_mut()
            .unwrap()
            .remove("realistic_voice");
        let older = older.to_string();
        assert!(!older.contains("realistic_voice"), "{older}");
        assert_eq!(
            decode_details(&older).map(|d| d.realistic_voice()),
            Ok(false)
        );
    }

    #[test]
    fn a_newer_version_is_refused_with_its_number() {
        let newer = encode_details(&details()).replace("\"version\": 1", "\"version\": 3");
        assert_eq!(decode(&newer), Err(PackageError::NewerVersion(3)));
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
        let package = encode_details(&details());
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
        let package =
            encode_details(&details()).replacen('{', "{\n  \"exported_by\": \"Bardo\",", 1);
        assert_eq!(decode_details(&package), Ok(details()));
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
