//! The item file (`.bardo-item`, #340): any library item (a narrator, a
//! product, a character, place or object, a template) as one JSON file the
//! user moves to another project, profile or machine. Opening it adds an
//! independent copy to the open project or to the global library.
//!
//! It carries the item and its pictures, as base64: a narrator's drawn look
//! (never a photo of the user: a narrator is written as its persona
//! package), a product's images and logo, a world item's drawn pictures in
//! a catalog style (never an imported picture, which may show a real
//! person, nor a look from a channel's notes, which belongs to that
//! channel). It never carries keys, ids, research or uses.
//!
//! The format is versioned: a reader refuses newer versions with a message
//! to update Bardo. A `.bardo-persona` package (ADR-0005) still opens.
//!
//! ```json
//! { "format": "bardo-item", "version": 1, "kind": "template",
//!   "template": { "kind": "script", "instructions": "...", "prompt": "..." } }
//! ```

use std::io::Read as _;
use std::path::Path;
use std::time::SystemTime;

use bardo_domain::{
    LibraryItem, Look, PictureOrigin, PictureProvenance, ProductDraft, ProductImage,
    ProductImageKind, ProductKind, ProductLogo, ProductOwnership, ProductPicture, ProjectId,
    Provider, ReferencePicture, ReferencePictureId, TemplateBody, TemplateKind, WorldItemDraft,
    WorldKind,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::{Bardo, SharingError, Text};

/// Names the format, so another app's JSON is not mistaken for an item.
pub const FORMAT: &str = "bardo-item";

/// The newest version this build writes and reads.
pub const VERSION: u64 = 1;

/// The file extension the save and open dialogs suggest.
pub const EXTENSION: &str = "bardo-item";

/// Pictures make a file a few megabytes; anything past this is no item
/// file, and is not read into memory.
pub const MAX_BYTES: u64 = 128 * 1024 * 1024;

/// Why a file cannot be opened as an item.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ItemFileError {
    /// Not JSON, not an item file, or missing required fields.
    #[error("not a Bardo item file")]
    NotAnItemFile,
    /// Made by a newer Bardo, in a format this build does not know.
    #[error("item file version {0} is newer than this Bardo reads ({VERSION})")]
    NewerVersion(u64),
    /// A known version whose item breaks its rules (an edited file).
    #[error("the item in the file is invalid")]
    Invalid,
}

impl ItemFileError {
    pub fn message(&self) -> Text {
        match self {
            ItemFileError::NotAnItemFile => Text::SharingNotAnItemFile,
            ItemFileError::NewerVersion(_) => Text::SharingNewerItemFile,
            ItemFileError::Invalid => Text::SharingInvalidItemFile,
        }
    }
}

#[derive(Deserialize)]
struct Header {
    format: String,
    version: u64,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u64,
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    narrator: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    product: Option<ProductV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    world: Option<WorldV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template: Option<TemplateV1>,
}

#[derive(Serialize, Deserialize)]
struct ProductV1 {
    name: String,
    kind: String,
    description: String,
    audience: String,
    facts: String,
    offer: String,
    link: String,
    call_to_action: String,
    ownership: String,
    must_say: String,
    never_say: String,
    #[serde(default)]
    images: Vec<ProductImageV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    logo: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ProductImageV1 {
    kind: String,
    image: String,
}

#[derive(Serialize, Deserialize)]
struct WorldV1 {
    kind: String,
    name: String,
    appearance: String,
    #[serde(default)]
    always: String,
    #[serde(default)]
    never: String,
    #[serde(default)]
    pictures: Vec<WorldPictureV1>,
}

#[derive(Serialize, Deserialize)]
struct WorldPictureV1 {
    /// A catalog style's code.
    look: String,
    image: String,
    provider: String,
    model: String,
    prompt: String,
}

#[derive(Serialize, Deserialize)]
struct TemplateV1 {
    kind: String,
    instructions: String,
    prompt: String,
}

fn encode_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_bytes(text: &str) -> Result<Vec<u8>, ItemFileError> {
    base64::engine::general_purpose::STANDARD
        .decode(text.as_bytes())
        .map_err(|_| ItemFileError::Invalid)
}

fn envelope(kind: &str) -> Envelope {
    Envelope {
        format: FORMAT.to_owned(),
        version: VERSION,
        kind: kind.to_owned(),
        narrator: None,
        product: None,
        world: None,
        template: None,
    }
}

/// The item's file, as pretty-printed JSON so a user can read what it
/// carries before sharing it.
pub(crate) fn write(app: &Bardo, item: LibraryItem) -> Result<String, SharingError> {
    let envelope = match item {
        LibraryItem::Narrator(id) => {
            let package = app.export_persona(id).map_err(|_| SharingError::NotFound)?;
            Envelope {
                narrator: Some(serde_json::from_str(&package).map_err(|_| SharingError::NotFound)?),
                ..envelope("narrator")
            }
        }
        LibraryItem::Product(id) => {
            let found = app.products.get(id)?.ok_or(SharingError::NotFound)?;
            let brief = &found.brief;
            let images = app
                .products
                .images(id)?
                .into_iter()
                .map(|image| ProductImageV1 {
                    kind: image.kind.code().to_owned(),
                    image: encode_bytes(image.picture.bytes()),
                })
                .collect();
            let logo = app
                .products
                .logo(id)?
                .map(|logo| encode_bytes(logo.picture.bytes()));
            Envelope {
                product: Some(ProductV1 {
                    name: brief.name().to_owned(),
                    kind: brief.kind().code().to_owned(),
                    description: brief.description().to_owned(),
                    audience: brief.audience().to_owned(),
                    facts: brief.facts().join("\n"),
                    offer: brief.offer().to_owned(),
                    link: brief.link().to_owned(),
                    call_to_action: brief.call_to_action().to_owned(),
                    ownership: brief.ownership().code().to_owned(),
                    must_say: brief.must_say().to_owned(),
                    never_say: brief.never_say().to_owned(),
                    images,
                    logo,
                }),
                ..envelope("product")
            }
        }
        LibraryItem::World(id) => {
            let book = &app.world_book;
            let found = book.world.character(id)?.ok_or(SharingError::NotFound)?;
            let folder = book.folder(found.owner_project);
            let mut pictures = Vec::new();
            for picture in book.world.pictures(id)? {
                // Kept, drawn, in a catalog style: an imported picture may
                // show a real person, and a notes look is its channel's.
                let (true, Some(provider), Look::Style(_)) =
                    (picture.kept, picture.provenance.provider, &picture.look)
                else {
                    continue;
                };
                let Ok(bytes) = book.files.read(&folder, &picture.file) else {
                    continue;
                };
                pictures.push(WorldPictureV1 {
                    look: picture.look.code().to_owned(),
                    image: encode_bytes(&bytes),
                    provider: provider.code().to_owned(),
                    model: picture.provenance.model.clone(),
                    prompt: picture.provenance.prompt.clone(),
                });
            }
            Envelope {
                world: Some(WorldV1 {
                    kind: found.kind.code().to_owned(),
                    name: found.details.name().to_owned(),
                    appearance: found.details.appearance().to_owned(),
                    always: found.details.always().to_owned(),
                    never: found.details.never().to_owned(),
                    pictures,
                }),
                ..envelope("world")
            }
        }
        LibraryItem::Template { project, kind } => {
            let version = app
                .template_in_use(kind, project)
                .map_err(|_| SharingError::NotFound)?
                .versions
                .into_iter()
                .next()
                .ok_or(SharingError::NotFound)?;
            Envelope {
                template: Some(TemplateV1 {
                    kind: kind.code().to_owned(),
                    instructions: version.body.instructions().to_owned(),
                    prompt: version.body.prompt().to_owned(),
                }),
                ..envelope("template")
            }
        }
    };
    let mut json = serde_json::to_string_pretty(&envelope).expect("an item file serializes");
    json.push('\n');
    Ok(json)
}

/// Reads the file at `path`, refusing one too large to be an item file.
pub(crate) fn read_file(path: &Path) -> Result<String, SharingError> {
    let unreadable = |error: std::io::Error| {
        tracing::warn!(%error, "could not read an item file");
        SharingError::Unreadable(error.to_string())
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(unreadable)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ItemFileError::NotAnItemFile.into());
    }
    String::from_utf8(bytes).map_err(|_| ItemFileError::NotAnItemFile.into())
}

/// A name free in the owner: `name`, else `name (imported)`, numbered.
fn free_name(app: &Bardo, name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let suffix = app.text(Text::PersonaImportedSuffix);
    crate::sharing::suggested_name(name, &suffix, taken)
}

/// Opens the file's text as an independent copy owned by `into`.
pub(crate) fn open(
    app: &Bardo,
    text: &str,
    into: Option<ProjectId>,
    confirmed: bool,
) -> Result<LibraryItem, SharingError> {
    let header: Header = serde_json::from_str(text).map_err(|_| ItemFileError::NotAnItemFile)?;
    if header.format == crate::persona_package::FORMAT {
        return open_narrator(app, text, into);
    }
    if header.format != FORMAT || header.version == 0 {
        return Err(ItemFileError::NotAnItemFile.into());
    }
    if header.version > VERSION {
        return Err(ItemFileError::NewerVersion(header.version).into());
    }
    let envelope: Envelope =
        serde_json::from_str(text).map_err(|_| ItemFileError::NotAnItemFile)?;
    match envelope.kind.as_str() {
        "narrator" => {
            let package = envelope.narrator.ok_or(ItemFileError::NotAnItemFile)?;
            open_narrator(app, &package.to_string(), into)
        }
        "product" => open_product(
            app,
            envelope.product.ok_or(ItemFileError::NotAnItemFile)?,
            into,
            confirmed,
        ),
        "world" => open_world(
            app,
            envelope.world.ok_or(ItemFileError::NotAnItemFile)?,
            into,
        ),
        "template" => open_template(
            app,
            envelope.template.ok_or(ItemFileError::NotAnItemFile)?,
            into,
        ),
        _ => Err(ItemFileError::NotAnItemFile.into()),
    }
}

fn open_narrator(
    app: &Bardo,
    package: &str,
    into: Option<ProjectId>,
) -> Result<LibraryItem, SharingError> {
    use crate::persona_package::PackageError;
    match app.import_persona_in(into, package) {
        Ok(persona) => Ok(LibraryItem::Narrator(persona.id)),
        Err(crate::PersonaError::Package(PackageError::NewerVersion(version))) => {
            Err(ItemFileError::NewerVersion(version).into())
        }
        Err(crate::PersonaError::Package(PackageError::NotAPackage)) => {
            Err(ItemFileError::NotAnItemFile.into())
        }
        Err(crate::PersonaError::ProjectNotFound) => Err(SharingError::ProjectNotFound),
        Err(crate::PersonaError::Repository(error)) => Err(error.into()),
        Err(_) => Err(ItemFileError::Invalid.into()),
    }
}

fn open_product(
    app: &Bardo,
    product: ProductV1,
    into: Option<ProjectId>,
    confirmed: bool,
) -> Result<LibraryItem, SharingError> {
    let images = product
        .images
        .iter()
        .map(|image| {
            let kind = ProductImageKind::from_code(&image.kind)
                .filter(|kind| ProductImageKind::ALL.contains(kind))
                .ok_or(ItemFileError::Invalid)?;
            let picture = ProductPicture::image(decode_bytes(&image.image)?)
                .map_err(|_| ItemFileError::Invalid)?;
            Ok((kind, picture))
        })
        .collect::<Result<Vec<_>, ItemFileError>>()?;
    if images.len() > bardo_domain::MAX_PRODUCT_IMAGES {
        return Err(ItemFileError::Invalid.into());
    }
    let logo = product
        .logo
        .as_deref()
        .map(|logo| ProductPicture::logo(decode_bytes(logo)?).map_err(|_| ItemFileError::Invalid))
        .transpose()?;
    if (logo.is_some() || !images.is_empty()) && !confirmed {
        return Err(SharingError::NotConfirmed);
    }
    let taken: Vec<String> = app
        .products
        .list(app.profile.id)?
        .into_iter()
        .filter(|other| other.owner_project == into)
        .map(|other| other.brief.name().to_lowercase())
        .collect();
    let name = free_name(app, product.name.trim(), |name| {
        taken.contains(&name.to_lowercase())
    });
    let draft = ProductDraft {
        name,
        kind: ProductKind::from_code(&product.kind).ok_or(ItemFileError::Invalid)?,
        description: product.description,
        audience: product.audience,
        facts: product.facts,
        offer: product.offer,
        link: product.link,
        call_to_action: product.call_to_action,
        ownership: ProductOwnership::from_code(&product.ownership).ok_or(ItemFileError::Invalid)?,
        must_say: product.must_say,
        never_say: product.never_say,
    };
    let created = app
        .create_product_in(into, draft)
        .map_err(|error| match error {
            crate::ProductError::Repository(error) => SharingError::Repository(error),
            crate::ProductError::ProjectNotFound => SharingError::ProjectNotFound,
            _ => ItemFileError::Invalid.into(),
        })?;
    let now = crate::publications::whole_millis(SystemTime::now());
    for (kind, picture) in images {
        app.products
            .add_image(&ProductImage::new(created.id, picture, kind, now))?;
    }
    if let Some(picture) = logo {
        app.products.save_logo(&ProductLogo {
            product: created.id,
            picture,
            confirmed_at: now,
        })?;
    }
    Ok(LibraryItem::Product(created.id))
}

fn open_world(
    app: &Bardo,
    world: WorldV1,
    into: Option<ProjectId>,
) -> Result<LibraryItem, SharingError> {
    let kind = WorldKind::from_code(&world.kind).ok_or(ItemFileError::Invalid)?;
    let mut pictures = Vec::new();
    for picture in world.pictures {
        let look = Look::from_parts(&picture.look, None, None)
            .filter(|look| matches!(look, Look::Style(_)))
            .ok_or(ItemFileError::Invalid)?;
        let provider: Provider = picture
            .provider
            .parse()
            .map_err(|_| ItemFileError::Invalid)?;
        let image = ProductPicture::image(decode_bytes(&picture.image)?)
            .map_err(|_| ItemFileError::Invalid)?;
        pictures.push((look, provider, picture.model, picture.prompt, image));
    }
    let book = &app.world_book;
    let folder = book.folder(into);
    if !pictures.is_empty() && !book.files.reachable(&folder) {
        return Err(SharingError::FolderUnreachable);
    }
    let taken: Vec<String> = book
        .world
        .characters(app.profile.id, into)?
        .into_iter()
        .filter(|other| other.kind == kind)
        .map(|other| other.details.name().to_lowercase())
        .collect();
    let name = free_name(app, world.name.trim(), |name| {
        taken.contains(&name.to_lowercase())
    });
    let created = app
        .create_world_item(
            into,
            kind,
            WorldItemDraft {
                name,
                appearance: world.appearance,
                always: world.always,
                never: world.never,
            },
        )
        .map_err(|error| match error {
            crate::WorldError::Repository(error) => SharingError::Repository(error),
            _ => ItemFileError::Invalid.into(),
        })?;
    let now = crate::publications::whole_millis(SystemTime::now());
    for (look, provider, model, prompt, image) in pictures {
        let id = ReferencePictureId::new();
        let file = format!("world-{id}.{}", image.format().extension());
        if let Err(error) = book.files.write(&folder, &file, image.bytes()) {
            tracing::warn!("an opened item's picture was not kept: {error}");
            continue;
        }
        book.save_replacing(
            into,
            &ReferencePicture {
                id,
                item: created.id,
                look,
                file,
                provenance: PictureProvenance {
                    provider: Some(provider),
                    model,
                    prompt,
                    generated_at: now,
                    job: None,
                    origin: PictureOrigin::Copied,
                    confirmed_at: None,
                },
                kept: true,
            },
        )?;
    }
    Ok(LibraryItem::World(created.id))
}

fn open_template(
    app: &Bardo,
    template: TemplateV1,
    into: Option<ProjectId>,
) -> Result<LibraryItem, SharingError> {
    let kind: TemplateKind = template.kind.parse().map_err(|_| ItemFileError::Invalid)?;
    TemplateBody::new(kind, &template.instructions, &template.prompt)
        .map_err(|_| ItemFileError::Invalid)?;
    if let Some(project) = into
        && app
            .templates
            .template_versions(app.profile.id, Some(project), kind)?
            .is_empty()
    {
        app.make_project_template(kind, project)
            .map_err(|_| SharingError::NotFound)?;
    }
    app.save_template_in(kind, into, (&template.instructions, &template.prompt), true)
        .map_err(|error| match error {
            crate::TemplateError::Repository(error) => SharingError::Repository(error),
            _ => ItemFileError::Invalid.into(),
        })?;
    Ok(LibraryItem::Template {
        project: into,
        kind,
    })
}

/// A file name for the item: its name without characters Windows refuses
/// in file names, plus the extension.
pub fn file_name(stem: &str) -> String {
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_end_matches('.');
    let stem = if stem.is_empty() { "item" } else { stem };
    format!("{stem}.{EXTENSION}")
}
