//! Moving settings between copies of Bardo (issue #162, ADR-0014): the
//! user ticks what goes into a password-protected settings file
//! (`settings_file`), and ticks again what to bring in when opening one.
//!
//! What can travel: provider API keys, network app credentials, the
//! preferences, budgets and changed prices, personas, and channels with
//! their network accounts. A network account's sign-in can travel too,
//! unticked by default: networks that rotate refresh tokens (X) sign the
//! other copy out once one of them refreshes.
//!
//! Bringing settings in adds what is new and replaces, by name, what is
//! already here: a persona or channel of the same name keeps its place
//! (its projects, the channels that use it) and takes the file's details.
//! Nothing else here changes.
//!
//! The contents are JSON. Every part is optional and read on its own: a
//! newer Bardo's file may carry parts or values this one does not know,
//! which are listed as not readable instead of failing the whole file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, AppCredentials, Budget, CaptionHighlight, ChannelDraft, ClipModelRef,
    ConnectedIdentity, ConnectionStatus, DecisionEngineId, LayoutId, Meter, MetricsSyncOnStart,
    Money, MotionDefault, Network, NetworkAccount, NetworkAccountDraft, NetworkConnection, Persona,
    Provider, Rate, Resolution, Score, ThumbnailLook, TokenSet, Transition, UiLanguage,
    UiThemePreference, VideoFormat, VoiceFlag,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::settings_file::{
    self, OpenedSettings, SettingsFileError, SettingsFileInfo, SettingsPassword,
};
use crate::{Bardo, KeyState, Text, persona_package};

/// One thing a settings file can carry. Personas, channels and sign-ins
/// are numbered in the list they come from: this copy's on export, the
/// file's on import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsItem {
    Key(Provider),
    AppCredentials(Network),
    Preferences,
    /// Budgets and changed prices.
    Costs,
    Persona(usize),
    Channel(usize),
    /// The network account's sign-in, in the channel numbered alike. It
    /// travels only with its channel.
    SignIn {
        channel: usize,
        network: Network,
    },
}

/// The parts of a settings file, in the order the screens list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsGroup {
    Keys,
    AppCredentials,
    Preferences,
    Personas,
    Channels,
    SignIns,
}

impl SettingsGroup {
    pub const ALL: [SettingsGroup; 6] = [
        SettingsGroup::Keys,
        SettingsGroup::AppCredentials,
        SettingsGroup::Preferences,
        SettingsGroup::Personas,
        SettingsGroup::Channels,
        SettingsGroup::SignIns,
    ];
}

impl SettingsItem {
    pub fn group(self) -> SettingsGroup {
        match self {
            SettingsItem::Key(_) => SettingsGroup::Keys,
            SettingsItem::AppCredentials(_) => SettingsGroup::AppCredentials,
            SettingsItem::Preferences | SettingsItem::Costs => SettingsGroup::Preferences,
            SettingsItem::Persona(_) => SettingsGroup::Personas,
            SettingsItem::Channel(_) => SettingsGroup::Channels,
            SettingsItem::SignIn { .. } => SettingsGroup::SignIns,
        }
    }

    /// Ticked unless the user says otherwise: everything but sign-ins.
    fn ticked_by_default(self) -> bool {
        !matches!(self, SettingsItem::SignIn { .. })
    }

    /// The item it travels with, if any.
    pub fn needs(self) -> Option<SettingsItem> {
        match self {
            SettingsItem::SignIn { channel, .. } => Some(SettingsItem::Channel(channel)),
            _ => None,
        }
    }
}

/// What bringing an item in does here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemEffect {
    Adds,
    /// The same key, persona, channel or sign-in is here: the file's
    /// replaces it.
    Replaces,
}

/// Why an item cannot be brought in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemProblem {
    /// Written by a newer Bardo, with values this one does not know, or
    /// edited by hand.
    NotReadable,
    /// The channel uses a video model this copy does not offer.
    ModelNotOffered,
}

/// One line of the export or import list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsChoice {
    pub item: SettingsItem,
    /// What the line names: a provider, a network, a persona, a channel
    /// (and its network, for a sign-in).
    pub label: String,
    /// Ticked when the list opens.
    pub ticked: bool,
    /// Import only.
    pub effect: Option<ItemEffect>,
    pub problem: Option<ItemProblem>,
}

/// Why an export or import did not happen.
#[derive(Debug, thiserror::Error)]
pub enum SettingsTransferError {
    #[error(transparent)]
    File(#[from] SettingsFileError),
    #[error(transparent)]
    Password(#[from] settings_file::PasswordError),
    #[error("nothing is ticked")]
    NothingChosen,
    #[error("could not read the file: {0}")]
    Read(std::io::Error),
    #[error("could not write the file: {0}")]
    Write(std::io::Error),
}

impl SettingsTransferError {
    pub fn message(&self) -> Text {
        match self {
            SettingsTransferError::File(SettingsFileError::NotASettingsFile) => {
                Text::SettingsFileNotOne
            }
            SettingsTransferError::File(SettingsFileError::NewerVersion(_)) => {
                Text::SettingsFileNewer
            }
            SettingsTransferError::File(SettingsFileError::WrongPasswordOrDamaged) => {
                Text::SettingsFileWrongPassword
            }
            SettingsTransferError::File(SettingsFileError::NoRandomness)
            | SettingsTransferError::Write(_) => Text::SettingsFileNotSaved,
            SettingsTransferError::Password(settings_file::PasswordError::TooShort) => {
                Text::SettingsFilePasswordShort
            }
            SettingsTransferError::Password(settings_file::PasswordError::DoNotMatch) => {
                Text::SettingsFilePasswordsDiffer
            }
            SettingsTransferError::NothingChosen => Text::SettingsFileNothingTicked,
            SettingsTransferError::Read(_) => Text::SettingsFileUnreadable,
        }
    }
}

/// The settings picked for a file, ready to seal and write off the UI
/// thread: deriving the key takes about half a second.
pub struct SettingsExport {
    contents: Zeroizing<Vec<u8>>,
    password: SettingsPassword,
    to: PathBuf,
}

impl SettingsExport {
    pub fn run(self) -> Result<PathBuf, SettingsTransferError> {
        let bytes = settings_file::seal(&self.contents, &self.password, SystemTime::now())?;
        write_file(&self.to, &bytes).map_err(SettingsTransferError::Write)?;
        tracing::info!("exported a settings file");
        Ok(self.to)
    }
}

/// A settings file to open, off the UI thread.
pub struct SettingsOpen {
    from: PathBuf,
    password: SettingsPassword,
}

impl SettingsOpen {
    pub fn run(self) -> Result<OpenedFile, SettingsTransferError> {
        let bytes = read_file(&self.from)?;
        open_bytes(&bytes, &self.password, self.from)
    }
}

/// What a settings file says before its password: refuses what is not one.
pub fn inspect_settings_file(path: &Path) -> Result<SettingsFileInfo, SettingsTransferError> {
    Ok(settings_file::inspect(&read_file(path)?)?)
}

/// An opened settings file, held until the user picks what to bring in.
pub struct OpenedFile {
    pub path: PathBuf,
    pub info: SettingsFileInfo,
    contents: Contents,
}

impl std::fmt::Debug for OpenedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedFile")
            .field("path", &self.path)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// What bringing settings in did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportOutcome {
    pub brought: Vec<SettingsChoice>,
    /// What could not be brought in, with why.
    pub failed: Vec<(SettingsChoice, Text)>,
}

/// Bringing settings in from a file, offered over the screens on the first
/// start of a new copy, or for the file chosen in the installer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsOffer {
    /// The file the installer was given: Bardo asks its password next.
    pub file: Option<PathBuf>,
}

/// The file name the save dialog offers, e.g.
/// `bardo-settings-20261017-153000.bardo-settings` (UTC).
pub fn settings_file_name(now: SystemTime) -> String {
    let at = jiff::Timestamp::try_from(now)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%Y%m%d-%H%M%S");
    format!("bardo-settings-{at}.{}", settings_file::EXTENSION)
}

fn read_file(path: &Path) -> Result<Vec<u8>, SettingsTransferError> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(SettingsTransferError::Read)?
        // One byte past the limit tells "too large" from "at the limit".
        .take(settings_file::MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(SettingsTransferError::Read)?;
    Ok(bytes)
}

/// Writes beside the target first, so a failed write never leaves half a
/// file where the user chose.
fn write_file(to: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let part = to.with_file_name(format!("{name}.part"));
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, to).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

fn open_bytes(
    bytes: &[u8],
    password: &SettingsPassword,
    path: PathBuf,
) -> Result<OpenedFile, SettingsTransferError> {
    let OpenedSettings { info, contents } = settings_file::open(bytes, password)?;
    // Sealed by Bardo, so it is its JSON; anything else is damage.
    let contents: Contents =
        serde_json::from_slice(&contents).map_err(|_| SettingsFileError::WrongPasswordOrDamaged)?;
    Ok(OpenedFile {
        path,
        info,
        contents,
    })
}

// The contents, as the file keeps them. Every part may be missing, and
// unknown fields are skipped, so files from other versions read.

#[derive(Serialize, Deserialize, Default)]
struct Contents {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    keys: Vec<KeyEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    app_credentials: Vec<AppEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preferences: Option<PreferencesEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    costs: Option<CostsEntry>,
    /// Each a persona package (`persona_package`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    personas: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channels: Vec<ChannelEntry>,
    /// Parts a newer Bardo wrote.
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct KeyEntry {
    provider: String,
    key: String,
}

impl Drop for KeyEntry {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
struct AppEntry {
    network: String,
    client_id: String,
    client_secret: String,
}

impl Drop for AppEntry {
    fn drop(&mut self) {
        self.client_id.zeroize();
        self.client_secret.zeroize();
    }
}

#[derive(Serialize, Deserialize, Default)]
struct PreferencesEntry {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    metrics_sync: Option<String>,
    #[serde(default)]
    cut_suggestion_floor: Option<u8>,
    #[serde(default)]
    screen_tours: Option<bool>,
    #[serde(default)]
    auto_backup: Option<bool>,
    #[serde(default)]
    update_check: Option<bool>,
    #[serde(default)]
    decision_engine: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct CostsEntry {
    #[serde(default)]
    budgets: Vec<BudgetEntry>,
    #[serde(default)]
    rates: Vec<RateEntry>,
}

#[derive(Serialize, Deserialize)]
struct BudgetEntry {
    provider: String,
    monthly_micros: u64,
}

#[derive(Serialize, Deserialize)]
struct RateEntry {
    provider: String,
    model: String,
    meter: String,
    price_micros: u64,
}

#[derive(Serialize, Deserialize)]
struct ChannelEntry {
    name: String,
    #[serde(default)]
    niche: String,
    #[serde(default)]
    themes: Vec<String>,
    #[serde(default)]
    aesthetic_notes: String,
    language: String,
    country: String,
    /// By name: the persona of that name here, if there is one.
    #[serde(default)]
    default_persona: Option<String>,
    #[serde(default)]
    clip_model: Option<ClipEntry>,
    /// Missing from files made before performed takes: the default one.
    #[serde(default)]
    take_model: Option<ClipEntry>,
    #[serde(default)]
    caption_style: Option<String>,
    /// Missing from files made before transitions: the default one.
    #[serde(default)]
    transition: Option<TransitionEntry>,
    /// Missing from files made before formats: long, as those channels
    /// made.
    #[serde(default)]
    format: Option<String>,
    /// Missing from files made before motion on stills: none, as those
    /// channels had.
    #[serde(default)]
    motion: Option<MotionEntry>,
    /// Missing from files made before the word highlight: off, as those
    /// channels' captions were.
    #[serde(default)]
    caption_highlight: Option<String>,
    /// Missing from files made before thumbnails: the default look.
    #[serde(default)]
    thumbnail_look: Option<String>,
    #[serde(default)]
    accounts: Vec<AccountEntry>,
}

#[derive(Serialize, Deserialize)]
struct ClipEntry {
    provider: String,
    model: String,
}

#[derive(Serialize, Deserialize)]
struct TransitionEntry {
    kind: String,
    frames: u32,
}

#[derive(Serialize, Deserialize)]
struct MotionEntry {
    pattern: String,
    strength: String,
}

#[derive(Serialize, Deserialize)]
struct AccountEntry {
    network: String,
    handle: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    description_footer: String,
    visibility: String,
    #[serde(default)]
    aspect: Option<String>,
    /// The short side, in pixels.
    #[serde(default)]
    resolution: Option<u32>,
    #[serde(default)]
    codec: Option<String>,
    /// As typed in the account's form; blank keeps the network's.
    #[serde(default)]
    bitrate: String,
    #[serde(default)]
    max_duration: String,
    #[serde(default)]
    loudness: String,
    /// Only when the user ticked the account's sign-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sign_in: Option<SignInEntry>,
}

#[derive(Serialize, Deserialize)]
struct SignInEntry {
    identity_id: String,
    identity_name: String,
    #[serde(default)]
    scopes: Vec<String>,
    status: String,
    connected_at: u64,
    #[serde(default)]
    refreshed_at: Option<u64>,
    /// As the secret store keeps them (`TokenSet::encode`).
    tokens: String,
}

impl Drop for SignInEntry {
    fn drop(&mut self) {
        self.tokens.zeroize();
    }
}

fn seconds(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn at_seconds(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

impl ChannelEntry {
    fn draft(&self, persona: Option<bardo_domain::PersonaId>) -> Option<ChannelDraft> {
        Some(ChannelDraft {
            name: self.name.clone(),
            niche: self.niche.clone(),
            themes: self.themes.clone(),
            aesthetic_notes: self.aesthetic_notes.clone(),
            language: self.language.parse().ok()?,
            country: self.country.parse().ok()?,
            default_persona: persona,
            clip_model: match &self.clip_model {
                Some(clip) => {
                    Some(ClipModelRef::new(clip.provider.parse().ok()?, &clip.model).ok()?)
                }
                None => None,
            },
            take_model: match &self.take_model {
                Some(take) => {
                    Some(ClipModelRef::new(take.provider.parse().ok()?, &take.model).ok()?)
                }
                None => None,
            },
            caption_style: match &self.caption_style {
                Some(style) => style.parse().ok()?,
                None => Default::default(),
            },
            caption_highlight: match &self.caption_highlight {
                Some(highlight) => highlight.parse().ok()?,
                None => CaptionHighlight::Off,
            },
            thumbnail_look: match &self.thumbnail_look {
                Some(look) => look.parse().ok()?,
                None => ThumbnailLook::default(),
            },
            transition: match &self.transition {
                Some(entry) => Transition::new(entry.kind.parse().ok()?, entry.frames),
                None => Transition::default(),
            },
            format: match &self.format {
                Some(format) => format.parse().ok()?,
                None => VideoFormat::Long,
            },
            motion: match &self.motion {
                Some(entry) => {
                    MotionDefault::new(entry.pattern.parse().ok()?, entry.strength.parse().ok()?)
                }
                None => MotionDefault::still(),
            },
        })
    }

    fn account(&self, network: Network) -> Option<&AccountEntry> {
        self.accounts
            .iter()
            .find(|account| account.network.parse::<Network>().ok() == Some(network))
    }

    fn networks(&self) -> Vec<Network> {
        self.accounts
            .iter()
            .filter_map(|account| account.network.parse().ok())
            .collect()
    }
}

impl AccountEntry {
    fn of(account: &NetworkAccount) -> Self {
        let details = &account.details;
        let metadata = details.metadata();
        let overrides = details.overrides();
        AccountEntry {
            network: account.network.code().to_owned(),
            handle: details.handle().to_owned(),
            language: metadata
                .language()
                .map(|language| language.code().to_owned()),
            tags: metadata.tags().to_vec(),
            description_footer: metadata.description_footer().to_owned(),
            visibility: metadata.visibility().code().to_owned(),
            aspect: overrides.aspect.map(|aspect| aspect.code().to_owned()),
            resolution: overrides
                .resolution
                .map(|resolution| resolution.short_side()),
            codec: overrides.codec.map(|codec| codec.code().to_owned()),
            bitrate: overrides.bitrate.map(|v| v.to_string()).unwrap_or_default(),
            max_duration: overrides
                .max_duration
                .map(|v| v.to_string())
                .unwrap_or_default(),
            loudness: overrides
                .loudness
                .map(|v| v.to_string())
                .unwrap_or_default(),
            sign_in: None,
        }
    }

    fn draft(&self) -> Option<NetworkAccountDraft> {
        Some(NetworkAccountDraft {
            handle: self.handle.clone(),
            language: match &self.language {
                Some(code) => Some(code.parse().ok()?),
                None => None,
            },
            tags: self.tags.clone(),
            description_footer: self.description_footer.clone(),
            visibility: self.visibility.parse().ok()?,
            aspect: match &self.aspect {
                Some(code) => Some(code.parse().ok()?),
                None => None,
            },
            resolution: match self.resolution {
                Some(pixels) => Some(Resolution::from_short_side(pixels)?),
                None => None,
            },
            codec: match &self.codec {
                Some(code) => Some(code.parse().ok()?),
                None => None,
            },
            bitrate: self.bitrate.clone(),
            max_duration: self.max_duration.clone(),
            loudness: self.loudness.clone(),
        })
    }
}

impl SignInEntry {
    fn of(connection: &NetworkConnection, tokens: &TokenSet) -> Option<Self> {
        Some(SignInEntry {
            identity_id: connection.identity.id.clone(),
            identity_name: connection.identity.name.clone(),
            scopes: connection.scopes.clone(),
            status: connection.status.code().to_owned(),
            connected_at: seconds(connection.connected_at),
            refreshed_at: connection.refreshed_at.map(seconds),
            tokens: tokens.encode().ok()?.to_string(),
        })
    }

    fn read(&self, account: &NetworkAccount) -> Option<(NetworkConnection, TokenSet)> {
        let tokens = TokenSet::decode(&self.tokens)?;
        let connection = NetworkConnection {
            account: account.id,
            owner: account.owner,
            status: ConnectionStatus::from_code(&self.status)?,
            identity: ConnectedIdentity {
                id: self.identity_id.clone(),
                name: self.identity_name.clone(),
            },
            scopes: self.scopes.clone(),
            expires_at: tokens.expires_at(),
            connected_at: at_seconds(self.connected_at),
            refreshed_at: self.refreshed_at.map(at_seconds),
        };
        Some((connection, tokens))
    }
}

impl Bardo {
    /// Bringing settings in, while offered.
    pub fn settings_offer(&self) -> Option<&SettingsOffer> {
        self.settings_offer.as_ref()
    }

    /// Offers bringing in the settings file the installer was given.
    pub fn offer_settings_file(&mut self, file: PathBuf) {
        self.settings_offer = Some(SettingsOffer { file: Some(file) });
    }

    /// Ends the offer: the user took it up or put it off.
    pub fn take_settings_offer(&mut self) -> Option<SettingsOffer> {
        self.settings_offer.take()
    }

    /// What this copy can put in a settings file, ticked as the export
    /// list opens: everything saved but the sign-ins.
    pub fn settings_to_export(&self) -> Vec<SettingsChoice> {
        let mut choices = Vec::new();
        let mut push = |item: SettingsItem, label: String| {
            choices.push(SettingsChoice {
                item,
                label,
                ticked: item.ticked_by_default(),
                effect: None,
                problem: None,
            });
        };
        for status in self.provider_keys() {
            if matches!(status.state, KeyState::Saved { .. }) {
                push(
                    SettingsItem::Key(status.provider),
                    self.text(Text::ProviderName(status.provider)).into_owned(),
                );
            }
        }
        for status in self.app_credentials() {
            if matches!(status.state, KeyState::Saved { .. }) {
                push(
                    SettingsItem::AppCredentials(status.network),
                    self.text(Text::NetworkName(status.network)).into_owned(),
                );
            }
        }
        push(
            SettingsItem::Preferences,
            self.text(Text::SettingsFilePreferences).into_owned(),
        );
        push(
            SettingsItem::Costs,
            self.text(Text::SettingsFileCosts).into_owned(),
        );
        for (index, persona) in self.personas().unwrap_or_default().iter().enumerate() {
            push(
                SettingsItem::Persona(index),
                persona.details.name().to_owned(),
            );
        }
        let channels = self.channels().unwrap_or_default();
        for (index, channel) in channels.iter().enumerate() {
            push(
                SettingsItem::Channel(index),
                channel.details.name().to_owned(),
            );
        }
        for (index, channel) in channels.iter().enumerate() {
            for account in self.network_accounts.list(channel.id).unwrap_or_default() {
                if self.saved_sign_in(&account).is_some() {
                    push(
                        SettingsItem::SignIn {
                            channel: index,
                            network: account.network,
                        },
                        self.sign_in_label(
                            channel.details.name(),
                            account.network,
                            account.details.handle(),
                        ),
                    );
                }
            }
        }
        choices
    }

    fn sign_in_label(&self, channel: &str, network: Network, handle: &str) -> String {
        self.text_with(
            Text::SettingsFileSignInLabel,
            &[
                ("handle", handle),
                ("network", &self.text(Text::NetworkName(network))),
                ("channel", channel),
            ],
        )
    }

    /// The ticked settings, ready to seal under `password` into `to`. A
    /// sign-in goes only with its channel.
    pub fn export_settings(
        &self,
        ticked: &[SettingsItem],
        password: SettingsPassword,
        to: PathBuf,
    ) -> Result<SettingsExport, SettingsTransferError> {
        if ticked.is_empty() {
            return Err(SettingsTransferError::NothingChosen);
        }
        let contents = self.contents(ticked);
        let json =
            Zeroizing::new(serde_json::to_vec(&contents).expect("settings contents serialize"));
        Ok(SettingsExport {
            contents: json,
            password,
            to,
        })
    }

    fn contents(&self, ticked: &[SettingsItem]) -> Contents {
        let has = |item: SettingsItem| ticked.contains(&item);
        let mut contents = Contents::default();
        for provider in Provider::with_keys() {
            if has(SettingsItem::Key(provider))
                && let Some(key) = self.saved_provider_key(provider)
            {
                contents.keys.push(KeyEntry {
                    provider: provider.code().to_owned(),
                    key: key.expose().to_owned(),
                });
            }
        }
        for network in Network::sign_in_networks() {
            if has(SettingsItem::AppCredentials(network))
                && let Some(credentials) = self.saved_app_credentials(network)
            {
                contents.app_credentials.push(AppEntry {
                    network: network.code().to_owned(),
                    client_id: credentials.client_id().to_owned(),
                    client_secret: credentials.client_secret().to_owned(),
                });
            }
        }
        if has(SettingsItem::Preferences) {
            let profile = &self.profile;
            contents.preferences = Some(PreferencesEntry {
                language: Some(profile.ui_language.tag().to_owned()),
                theme: Some(profile.ui_theme.code()),
                layout: Some(profile.ui_layout.code().to_owned()),
                metrics_sync: Some(profile.metrics_sync.code().to_owned()),
                cut_suggestion_floor: Some(profile.cut_suggestion_floor.value()),
                screen_tours: Some(profile.offer_screen_tours),
                auto_backup: Some(profile.auto_backup),
                update_check: Some(profile.update_check),
                decision_engine: Some(profile.decision_engine.code().to_owned()),
            });
        }
        if has(SettingsItem::Costs) {
            let costs = &self.cost_book.costs;
            let owner = self.profile.id;
            contents.costs = Some(CostsEntry {
                budgets: costs
                    .budgets(owner)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|budget| BudgetEntry {
                        provider: budget.provider.code().to_owned(),
                        monthly_micros: budget.monthly.micros(),
                    })
                    .collect(),
                rates: costs
                    .rate_changes(owner)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|rate| RateEntry {
                        provider: rate.provider.code().to_owned(),
                        model: rate.model,
                        meter: rate.meter.code().to_owned(),
                        price_micros: rate.price.micros(),
                    })
                    .collect(),
            });
        }
        let personas = self.personas().unwrap_or_default();
        for (index, persona) in personas.iter().enumerate() {
            if has(SettingsItem::Persona(index)) {
                // Without its look: the settings file stays small, and a
                // look travels in the persona's own package.
                let package = persona_package::encode(&persona.details, None);
                if let Ok(value) = serde_json::from_str(&package) {
                    contents.personas.push(value);
                }
            }
        }
        for (index, channel) in self.channels().unwrap_or_default().iter().enumerate() {
            if !has(SettingsItem::Channel(index)) {
                continue;
            }
            let details = &channel.details;
            let accounts = self
                .network_accounts
                .list(channel.id)
                .unwrap_or_default()
                .iter()
                .map(|account| {
                    let mut entry = AccountEntry::of(account);
                    if has(SettingsItem::SignIn {
                        channel: index,
                        network: account.network,
                    }) {
                        entry.sign_in = self
                            .saved_sign_in(account)
                            .and_then(|(connection, tokens)| SignInEntry::of(&connection, &tokens));
                    }
                    entry
                })
                .collect();
            contents.channels.push(ChannelEntry {
                name: details.name().to_owned(),
                niche: details.niche().to_owned(),
                themes: details.themes().to_vec(),
                aesthetic_notes: details.aesthetic_notes().to_owned(),
                language: details.language().code().to_owned(),
                country: details.country().code().to_owned(),
                default_persona: details.default_persona().and_then(|id| {
                    personas
                        .iter()
                        .find(|persona| persona.id == id)
                        .map(|persona| persona.details.name().to_owned())
                }),
                clip_model: details.clip_model().map(|model| ClipEntry {
                    provider: model.provider().code().to_owned(),
                    model: model.model().to_owned(),
                }),
                take_model: details.take_model().map(|model| ClipEntry {
                    provider: model.provider().code().to_owned(),
                    model: model.model().to_owned(),
                }),
                caption_style: Some(details.caption_style().code().to_owned()),
                caption_highlight: Some(details.caption_highlight().code().to_owned()),
                thumbnail_look: Some(details.thumbnail_look().code().to_owned()),
                transition: Some(TransitionEntry {
                    kind: details.transition().kind.code().to_owned(),
                    frames: details.transition().frames(),
                }),
                format: Some(details.format().code().to_owned()),
                motion: Some(MotionEntry {
                    pattern: details.motion().pattern.code().to_owned(),
                    strength: details.motion().strength.code().to_owned(),
                }),
                accounts,
            });
        }
        contents
    }

    /// Opens a settings file with its password. Run it off the UI thread.
    pub fn open_settings_file(&self, from: PathBuf, password: SettingsPassword) -> SettingsOpen {
        SettingsOpen { from, password }
    }

    /// What an opened file can bring in, and what each item does here.
    pub fn settings_to_import(&self, file: &OpenedFile) -> Vec<SettingsChoice> {
        let contents = &file.contents;
        let mut choices = Vec::new();
        let mut push = |item: SettingsItem,
                        label: String,
                        effect: ItemEffect,
                        problem: Option<ItemProblem>| {
            choices.push(SettingsChoice {
                item,
                label,
                ticked: problem.is_none() && item.ticked_by_default(),
                effect: Some(effect),
                problem,
            });
        };
        let replaces = |here: bool| {
            if here {
                ItemEffect::Replaces
            } else {
                ItemEffect::Adds
            }
        };
        for entry in &contents.keys {
            match entry.provider.parse::<Provider>() {
                Ok(provider) if provider.has_key() => push(
                    SettingsItem::Key(provider),
                    self.text(Text::ProviderName(provider)).into_owned(),
                    replaces(self.provider_key(provider).state != KeyState::NotSet),
                    ApiKey::parse(provider, &entry.key)
                        .err()
                        .map(|_| ItemProblem::NotReadable),
                ),
                _ => {}
            }
        }
        for entry in &contents.app_credentials {
            match entry.network.parse::<Network>() {
                Ok(network) if network.signs_in() => {
                    let saved = self.app_credentials().iter().any(|status| {
                        status.network == network && status.state != KeyState::NotSet
                    });
                    push(
                        SettingsItem::AppCredentials(network),
                        self.text(Text::NetworkName(network)).into_owned(),
                        replaces(saved),
                        AppCredentials::parse(network, &entry.client_id, &entry.client_secret)
                            .err()
                            .map(|_| ItemProblem::NotReadable),
                    );
                }
                _ => {}
            }
        }
        if contents.preferences.is_some() {
            push(
                SettingsItem::Preferences,
                self.text(Text::SettingsFilePreferences).into_owned(),
                ItemEffect::Replaces,
                None,
            );
        }
        if contents.costs.is_some() {
            push(
                SettingsItem::Costs,
                self.text(Text::SettingsFileCosts).into_owned(),
                ItemEffect::Replaces,
                None,
            );
        }
        let personas = self.personas().unwrap_or_default();
        for (index, value) in contents.personas.iter().enumerate() {
            let decoded =
                persona_package::decode(&value.to_string()).map(|package| package.details);
            let label = match &decoded {
                Ok(details) => details.name().to_owned(),
                Err(_) => value["persona"]["name"].as_str().unwrap_or("?").to_owned(),
            };
            let here = personas
                .iter()
                .any(|persona| persona.details.same_name(&label));
            push(
                SettingsItem::Persona(index),
                label,
                replaces(here),
                decoded.err().map(|_| ItemProblem::NotReadable),
            );
        }
        let channels = self.channels().unwrap_or_default();
        for (index, entry) in contents.channels.iter().enumerate() {
            let here = channels
                .iter()
                .any(|channel| channel.details.same_name(&entry.name));
            let problem = match entry.draft(None) {
                None => Some(ItemProblem::NotReadable),
                Some(draft) => match (draft.clip_model, draft.take_model) {
                    (Some(model), _) if !self.offers_clip_model(&model) => {
                        Some(ItemProblem::ModelNotOffered)
                    }
                    (_, Some(model)) if !self.offers_take_model(&model) => {
                        Some(ItemProblem::ModelNotOffered)
                    }
                    _ => entry
                        .accounts
                        .iter()
                        .any(|account| {
                            account.network.parse::<Network>().is_err() || account.draft().is_none()
                        })
                        .then_some(ItemProblem::NotReadable),
                },
            };
            push(
                SettingsItem::Channel(index),
                entry.name.clone(),
                replaces(here),
                problem,
            );
        }
        for (index, entry) in contents.channels.iter().enumerate() {
            let here = channels
                .iter()
                .find(|channel| channel.details.same_name(&entry.name));
            for account in &entry.accounts {
                let Some(sign_in) = &account.sign_in else {
                    continue;
                };
                let Ok(network) = account.network.parse::<Network>() else {
                    continue;
                };
                let connected = here.is_some_and(|channel| {
                    self.network_accounts
                        .list(channel.id)
                        .unwrap_or_default()
                        .iter()
                        .any(|existing| {
                            existing.network == network && self.saved_sign_in(existing).is_some()
                        })
                });
                let readable = TokenSet::decode(&sign_in.tokens).is_some()
                    && ConnectionStatus::from_code(&sign_in.status).is_some()
                    && network.signs_in();
                push(
                    SettingsItem::SignIn {
                        channel: index,
                        network,
                    },
                    self.sign_in_label(&entry.name, network, &account.handle),
                    replaces(connected),
                    (!readable).then_some(ItemProblem::NotReadable),
                );
            }
        }
        choices
    }

    /// The parts of the file this Bardo does not know, by their names in
    /// the file: a newer Bardo wrote them.
    pub fn settings_not_read(&self, file: &OpenedFile) -> Vec<String> {
        file.contents.unknown.keys().cloned().collect()
    }

    /// Brings the ticked items in. Each goes on its own: one that fails
    /// is listed and the others still come in.
    pub fn import_settings(&mut self, file: &OpenedFile, ticked: &[SettingsItem]) -> ImportOutcome {
        let choices = self.settings_to_import(file);
        let chosen = |item: SettingsItem| {
            ticked.contains(&item) && item.needs().is_none_or(|needed| ticked.contains(&needed))
        };
        let mut outcome = ImportOutcome::default();
        let mut record = |choice: &SettingsChoice, result: Result<(), Text>| match result {
            Ok(()) => outcome.brought.push(choice.clone()),
            Err(text) => outcome.failed.push((choice.clone(), text)),
        };
        let contents = &file.contents;
        // Preferences first: the language decides how the rest is named.
        for choice in &choices {
            if !chosen(choice.item) || choice.problem.is_some() {
                continue;
            }
            let result = match choice.item {
                SettingsItem::Preferences => contents
                    .preferences
                    .as_ref()
                    .map_or(Ok(()), |entry| self.bring_preferences(entry)),
                SettingsItem::Costs => contents
                    .costs
                    .as_ref()
                    .map_or(Ok(()), |entry| self.bring_costs(entry)),
                SettingsItem::Key(provider) => self.bring_key(contents, provider),
                SettingsItem::AppCredentials(network) => {
                    self.bring_app_credentials(contents, network)
                }
                SettingsItem::Persona(index) => self.bring_persona(&contents.personas[index]),
                _ => continue,
            };
            record(choice, result);
        }
        // Channels after personas, which their default persona names.
        for choice in &choices {
            let SettingsItem::Channel(index) = choice.item else {
                continue;
            };
            if !chosen(choice.item) || choice.problem.is_some() {
                continue;
            }
            let entry = &contents.channels[index];
            let signed_in: Vec<Network> = choices
                .iter()
                .filter_map(|other| match other.item {
                    SettingsItem::SignIn { channel, network }
                        if channel == index && other.problem.is_none() && chosen(other.item) =>
                    {
                        Some(network)
                    }
                    _ => None,
                })
                .collect();
            match self.bring_channel(entry, &signed_in) {
                Ok(failed_sign_ins) => {
                    record(choice, Ok(()));
                    for other in &choices {
                        if let SettingsItem::SignIn { channel, network } = other.item
                            && channel == index
                            && signed_in.contains(&network)
                        {
                            if failed_sign_ins.contains(&network) {
                                record(other, Err(Text::SettingsFileItemFailed));
                            } else {
                                record(other, Ok(()));
                            }
                        }
                    }
                }
                Err(text) => record(choice, Err(text)),
            }
        }
        tracing::info!(
            brought = outcome.brought.len(),
            failed = outcome.failed.len(),
            "imported a settings file"
        );
        outcome
    }

    fn bring_preferences(&mut self, entry: &PreferencesEntry) -> Result<(), Text> {
        let failed = |_| Text::SettingsFileItemFailed;
        if let Some(language) = entry
            .language
            .as_deref()
            .and_then(|tag| tag.parse::<UiLanguage>().ok())
        {
            self.set_ui_language(language).map_err(failed)?;
        }
        if let Some(code) = &entry.theme {
            let theme = UiThemePreference::from_code_or_default(code);
            if theme.code() == *code {
                self.set_ui_theme(theme).map_err(failed)?;
            }
        }
        if let Some(code) = &entry.layout {
            let layout = LayoutId::from_code_or_default(code);
            if layout.code() == code {
                self.set_ui_layout(layout).map_err(failed)?;
            }
        }
        if let Some(code) = &entry.metrics_sync {
            let sync = MetricsSyncOnStart::from_code_or_default(code);
            if sync.code() == code {
                self.set_metrics_sync_on_start(sync).map_err(failed)?;
            }
        }
        if let Some(floor) = entry.cut_suggestion_floor {
            self.set_cut_suggestion_floor(Score::new(floor))
                .map_err(failed)?;
        }
        if let Some(on) = entry.screen_tours {
            self.set_offer_screen_tours(on).map_err(failed)?;
        }
        if let Some(on) = entry.auto_backup {
            self.set_auto_backup(on).map_err(failed)?;
        }
        if let Some(on) = entry.update_check {
            self.set_update_check(on).map_err(failed)?;
        }
        // An engine this copy does not offer keeps the current one.
        if let Some(engine) = entry
            .decision_engine
            .as_deref()
            .and_then(|code| code.parse::<DecisionEngineId>().ok())
            .filter(|engine| self.decision_engines().contains(engine))
        {
            self.set_decision_engine(engine).map_err(failed)?;
        }
        Ok(())
    }

    fn bring_costs(&self, entry: &CostsEntry) -> Result<(), Text> {
        let costs = &self.cost_book.costs;
        let owner = self.profile.id;
        let mut all = true;
        for budget in &entry.budgets {
            let Ok(provider) = budget.provider.parse::<Provider>() else {
                all = false;
                continue;
            };
            let monthly = Money::from_micros(budget.monthly_micros);
            if !provider.is_paid() || monthly > Money::MAX {
                all = false;
                continue;
            }
            if costs
                .save_budget(owner, &Budget { provider, monthly })
                .is_err()
            {
                return Err(Text::SettingsFileItemFailed);
            }
        }
        for rate in &entry.rates {
            let read = rate
                .provider
                .parse::<Provider>()
                .ok()
                .zip(rate.meter.parse::<Meter>().ok())
                .and_then(|(provider, meter)| Rate::new(provider, &rate.model, meter, "0").ok());
            let price = Money::from_micros(rate.price_micros);
            let Some(read) = read.filter(|_| price <= Money::MAX) else {
                all = false;
                continue;
            };
            if costs.save_rate(owner, &Rate { price, ..read }).is_err() {
                return Err(Text::SettingsFileItemFailed);
            }
        }
        if all {
            Ok(())
        } else {
            Err(Text::SettingsFileSomeNotRead)
        }
    }

    fn bring_key(&mut self, contents: &Contents, provider: Provider) -> Result<(), Text> {
        let entry = contents
            .keys
            .iter()
            .find(|entry| entry.provider.parse::<Provider>().ok() == Some(provider))
            .ok_or(Text::SettingsFileItemFailed)?;
        let key = ApiKey::parse(provider, &entry.key).map_err(|_| Text::SettingsFileItemFailed)?;
        self.keep_provider_key(provider, &key)
            .map_err(|_| Text::SettingsFileItemFailed)
    }

    fn bring_app_credentials(&mut self, contents: &Contents, network: Network) -> Result<(), Text> {
        let entry = contents
            .app_credentials
            .iter()
            .find(|entry| entry.network.parse::<Network>().ok() == Some(network))
            .ok_or(Text::SettingsFileItemFailed)?;
        let credentials = AppCredentials::parse(network, &entry.client_id, &entry.client_secret)
            .map_err(|_| Text::SettingsFileItemFailed)?;
        self.keep_app_credentials(network, &credentials)
            .map_err(|_| Text::SettingsFileItemFailed)
    }

    fn bring_persona(&self, value: &serde_json::Value) -> Result<(), Text> {
        let failed = Text::SettingsFileItemFailed;
        let details = persona_package::decode(&value.to_string())
            .map_err(|_| failed)?
            .details;
        let existing = self
            .personas
            .list(self.profile.id)
            .map_err(|_| failed)?
            .into_iter()
            .find(|persona| persona.details.same_name(details.name()));
        let voice_flag = VoiceFlag::from_lookup(self.voice_lookup(details.voice()));
        let persona = match existing {
            Some(existing) => Persona {
                details,
                voice_flag,
                ..existing
            },
            None => Persona {
                voice_flag,
                ..Persona::new(self.profile.id, details)
            },
        };
        self.personas.save(&persona).map_err(|_| failed)
    }

    /// Brings the channel and its accounts in, and the sign-ins of
    /// `signed_in`; returns the sign-ins that failed.
    fn bring_channel(
        &self,
        entry: &ChannelEntry,
        signed_in: &[Network],
    ) -> Result<Vec<Network>, Text> {
        let failed = Text::SettingsFileItemFailed;
        let persona = match &entry.default_persona {
            Some(name) => self
                .personas()
                .map_err(|_| failed)?
                .into_iter()
                .find(|persona| persona.details.same_name(name))
                .map(|persona| persona.id),
            None => None,
        };
        let draft = entry.draft(persona).ok_or(Text::SettingsFileItemNotRead)?;
        let existing = self
            .channels()
            .map_err(|_| failed)?
            .into_iter()
            .find(|channel| channel.details.same_name(&entry.name));
        let channel = match existing {
            Some(existing) => self.update_channel(existing.id, draft),
            None => self.create_channel(draft),
        }
        .map_err(|_| failed)?;
        let mut sign_ins_failed = Vec::new();
        for network in entry.networks() {
            let Some(account_entry) = entry.account(network) else {
                continue;
            };
            let Some(draft) = account_entry.draft() else {
                sign_ins_failed.push(network);
                continue;
            };
            let existing = self
                .network_accounts
                .list(channel.id)
                .unwrap_or_default()
                .into_iter()
                .find(|account| account.network == network);
            let account = match existing {
                Some(existing) => self.update_network_account(existing.id, draft),
                None => self.add_network_account(channel.id, network, draft),
            };
            let Ok(account) = account else {
                sign_ins_failed.push(network);
                continue;
            };
            if !signed_in.contains(&network) {
                continue;
            }
            let kept = account_entry
                .sign_in
                .as_ref()
                .and_then(|sign_in| sign_in.read(&account))
                .is_some_and(|(connection, tokens)| {
                    self.keep_sign_in(&account, connection, &tokens).is_ok()
                });
            if !kept {
                sign_ins_failed.push(network);
            }
        }
        Ok(sign_ins_failed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        ConnectionSecrets as _, ContentLanguage, GenerationPresets, NetworkConnectionRepository,
        PersonaDraft, SecretStore as _, UiTheme, VoiceRef,
    };

    use super::*;
    use crate::scenes::tests::Harness;
    use crate::settings_file::tests::seal_cheaply;

    const META_ID: &str = "1234567890123456";
    const META_SECRET: &str = concat!("0123456789abcdef", "0123456789abcdef");
    const PASSWORD: &str = "correct horse";

    fn password() -> SettingsPassword {
        SettingsPassword::typed(PASSWORD)
    }

    /// A copy of Bardo set up with one of everything a file carries.
    fn set_up(h: &Harness) -> Bardo {
        let mut app = h.start();
        app.save_app_credentials(Network::InstagramReels, META_ID, META_SECRET)
            .unwrap();
        app.set_ui_language(UiLanguage::PtBr).unwrap();
        app.set_ui_theme(UiThemePreference::Fixed(UiTheme::Slate))
            .unwrap();
        app.set_ui_layout(LayoutId::Studio).unwrap();
        app.set_auto_backup(false).unwrap();
        app.set_budget(Provider::Claude, "25").unwrap();
        let persona = app
            .create_persona(PersonaDraft {
                name: "Narradora".into(),
                voice: Some(VoiceRef::new(Provider::ElevenLabs, "voice01", "Ana").unwrap()),
                tone: "Calma.".into(),
                script_style: "Frases curtas.".into(),
                presets: GenerationPresets::default(),
                realistic_voice: false,
            })
            .unwrap();
        let channel = app
            .create_channel(ChannelDraft {
                name: "Arquivos do Espaço".into(),
                niche: "Espaço".into(),
                themes: vec!["Sondas".into()],
                language: ContentLanguage::Portuguese,
                default_persona: Some(persona.id),
                transition: Transition::new(bardo_domain::TransitionKind::WipeRight, 21),
                caption_highlight: CaptionHighlight::Cyan,
                thumbnail_look: ThumbnailLook::Banner,
                motion: MotionDefault::new(
                    bardo_domain::MotionPattern::Same(bardo_domain::MotionKind::PanLeft),
                    bardo_domain::MotionStrength::Subtle,
                ),
                format: VideoFormat::Short,
                ..ChannelDraft::default()
            })
            .unwrap();
        let account = app
            .add_network_account(
                channel.id,
                Network::InstagramReels,
                NetworkAccountDraft {
                    handle: "arquivos".into(),
                    tags: vec!["espaço".into()],
                    bitrate: "8".into(),
                    ..NetworkAccountDraft::default()
                },
            )
            .unwrap();
        let tokens = TokenSet::decode("bardo-tokens-1\n4102444800\naccess-token-0001\n").unwrap();
        h.connection_secrets
            .set_tokens(app.profile().id, account.id, &tokens)
            .unwrap();
        NetworkConnectionRepository::save(
            &*h.db,
            &NetworkConnection {
                account: account.id,
                owner: app.profile().id,
                status: ConnectionStatus::Connected,
                identity: ConnectedIdentity {
                    id: "17841400000000000".into(),
                    name: "@arquivos".into(),
                },
                scopes: vec!["instagram_content_publish".into()],
                expires_at: tokens.expires_at(),
                connected_at: at_seconds(1_792_000_000),
                refreshed_at: None,
            },
        )
        .unwrap();
        app
    }

    #[test]
    fn a_channel_from_a_file_made_before_formats_makes_long_videos() {
        let entry: ChannelEntry =
            serde_json::from_str(r#"{"name": "Old", "language": "en", "country": "US"}"#).unwrap();
        assert_eq!(entry.draft(None).unwrap().format, VideoFormat::Long);
    }

    #[test]
    fn a_channel_from_a_file_made_before_thumbnails_gets_the_default_look() {
        let entry: ChannelEntry =
            serde_json::from_str(r#"{"name": "Old", "language": "en", "country": "US"}"#).unwrap();
        assert_eq!(
            entry.draft(None).unwrap().thumbnail_look,
            ThumbnailLook::Bold
        );
    }

    #[test]
    fn a_channel_from_a_file_made_before_the_highlight_lights_no_word() {
        let entry: ChannelEntry =
            serde_json::from_str(r#"{"name": "Old", "language": "en", "country": "US"}"#).unwrap();
        assert_eq!(
            entry.draft(None).unwrap().caption_highlight,
            CaptionHighlight::Off
        );
    }

    #[test]
    fn a_channel_from_a_file_made_before_motion_keeps_its_stills_still() {
        let entry: ChannelEntry =
            serde_json::from_str(r#"{"name": "Old", "language": "en", "country": "US"}"#).unwrap();
        assert_eq!(entry.draft(None).unwrap().motion, MotionDefault::still());
    }

    fn items(choices: &[SettingsChoice]) -> Vec<SettingsItem> {
        choices.iter().map(|choice| choice.item).collect()
    }

    fn ticked(choices: &[SettingsChoice]) -> Vec<SettingsItem> {
        choices
            .iter()
            .filter(|choice| choice.ticked)
            .map(|choice| choice.item)
            .collect()
    }

    fn export(app: &Bardo, items: &[SettingsItem], dir: &Path) -> PathBuf {
        app.export_settings(
            items,
            SettingsPassword::choose(PASSWORD, PASSWORD).unwrap(),
            dir.join("settings.bardo-settings"),
        )
        .unwrap()
        .run()
        .unwrap()
    }

    fn open(app: &Bardo, path: &Path) -> OpenedFile {
        app.open_settings_file(path.to_owned(), password())
            .run()
            .unwrap()
    }

    fn sign_in_of(app: &Bardo) -> Option<(NetworkConnection, TokenSet)> {
        let channel = app
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.details.same_name("Arquivos do Espaço"))?;
        let account = app
            .network_accounts
            .list(channel.id)
            .unwrap()
            .into_iter()
            .find(|account| account.network == Network::InstagramReels)?;
        app.saved_sign_in(&account)
    }

    #[test]
    fn a_new_copy_offers_a_settings_file_once() {
        let h = Harness::new();
        let mut first = h.start();
        assert_eq!(first.settings_offer(), Some(&SettingsOffer::default()));
        assert_eq!(first.take_settings_offer(), Some(SettingsOffer::default()));
        assert_eq!(first.settings_offer(), None);

        let mut again = h.start();
        assert_eq!(again.settings_offer(), None, "only on the first start");
        let file = PathBuf::from("bardo.bardo-settings");
        again
            .apply_setup_choices(&bardo_storage::SetupChoices {
                settings_file: Some(file.clone()),
                ..bardo_storage::SetupChoices::default()
            })
            .unwrap();
        assert_eq!(
            again.settings_offer(),
            Some(&SettingsOffer { file: Some(file) }),
            "the installer's file"
        );
    }

    #[test]
    fn the_export_list_ticks_everything_but_sign_ins() {
        let h = Harness::new();
        let app = set_up(&h);
        let choices = app.settings_to_export();
        let all = items(&choices);
        assert!(all.contains(&SettingsItem::Key(Provider::Claude)));
        assert!(all.contains(&SettingsItem::AppCredentials(Network::InstagramReels)));
        assert!(!all.contains(&SettingsItem::AppCredentials(Network::YouTube)));
        let sign_in = SettingsItem::SignIn {
            channel: 0,
            network: Network::InstagramReels,
        };
        assert!(all.contains(&sign_in));
        assert_eq!(sign_in.needs(), Some(SettingsItem::Channel(0)));
        let ticked = ticked(&choices);
        assert!(!ticked.contains(&sign_in));
        assert_eq!(ticked.len(), all.len() - 1);
        let label = &choices.iter().find(|c| c.item == sign_in).unwrap().label;
        assert_eq!(label, "arquivos no Instagram Reels (Arquivos do Espaço)");
    }

    #[test]
    fn what_was_ticked_arrives_in_another_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = Harness::new();
        let app = set_up(&source);
        let path = export(&app, &ticked(&app.settings_to_export()), dir.path());

        let target = Harness::new();
        let mut other = target.start();
        let file = open(&other, &path);
        assert_eq!(file.info.bardo_version, env!("CARGO_PKG_VERSION"));
        let preferences = file.contents.preferences.as_ref().unwrap();
        assert_eq!(preferences.decision_engine.as_deref(), Some("jev"));
        let choices = other.settings_to_import(&file);
        let channel = choices
            .iter()
            .find(|choice| choice.item == SettingsItem::Channel(0))
            .unwrap();
        assert_eq!(channel.label, "Arquivos do Espaço");
        assert_eq!(channel.effect, Some(ItemEffect::Adds));
        // The other copy has its own Claude key: the file's replaces it.
        let claude = choices
            .iter()
            .find(|choice| choice.item == SettingsItem::Key(Provider::Claude))
            .unwrap();
        assert_eq!(claude.effect, Some(ItemEffect::Replaces));
        // Not ticked on export, so not in the file.
        assert!(
            !choices
                .iter()
                .any(|choice| matches!(choice.item, SettingsItem::SignIn { .. }))
        );

        let outcome = other.import_settings(&file, &ticked(&choices));
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(outcome.brought.len(), choices.len());

        let owner = other.profile().id;
        assert_eq!(
            target
                .connection_secrets
                .app_credentials(owner, Network::InstagramReels)
                .unwrap()
                .unwrap()
                .client_secret(),
            META_SECRET
        );
        assert_eq!(
            target
                .secrets
                .get(owner, Provider::Claude)
                .unwrap()
                .unwrap(),
            source
                .secrets
                .get(app.profile().id, Provider::Claude)
                .unwrap()
                .unwrap()
        );
        assert_eq!(other.profile().ui_language, UiLanguage::PtBr);
        assert_eq!(other.ui_theme(), UiThemePreference::Fixed(UiTheme::Slate));
        assert_eq!(other.ui_layout(), LayoutId::Studio);
        assert!(!other.profile().auto_backup);
        let budgets = other.cost_book.costs.budgets(owner).unwrap();
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0].monthly, Money::from_cents(2500));
        let narrator = other
            .personas()
            .unwrap()
            .into_iter()
            .find(|persona| persona.details.same_name("Narradora"))
            .unwrap();
        assert_eq!(narrator.details.tone(), "Calma.");
        let channel = other
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.details.same_name("Arquivos do Espaço"))
            .unwrap();
        assert_eq!(channel.details.default_persona(), Some(narrator.id));
        assert_eq!(channel.details.themes(), ["Sondas"]);
        assert_eq!(
            channel.details.transition(),
            Transition::new(bardo_domain::TransitionKind::WipeRight, 21)
        );
        assert_eq!(channel.details.format(), VideoFormat::Short);
        assert_eq!(channel.details.caption_highlight(), CaptionHighlight::Cyan);
        assert_eq!(channel.details.thumbnail_look(), ThumbnailLook::Banner);
        assert_eq!(
            channel.details.motion(),
            MotionDefault::new(
                bardo_domain::MotionPattern::Same(bardo_domain::MotionKind::PanLeft),
                bardo_domain::MotionStrength::Subtle,
            )
        );
        let accounts = other.network_accounts.list(channel.id).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].details.handle(), "arquivos");
        assert_eq!(accounts[0].details.metadata().tags(), ["espaço"]);
        assert_eq!(
            accounts[0]
                .details
                .overrides()
                .bitrate
                .map(|b| b.to_string()),
            Some("8".to_owned())
        );
        assert!(sign_in_of(&other).is_none(), "sign-ins stay unless ticked");
    }

    #[test]
    fn a_ticked_sign_in_travels_with_its_channel() {
        let dir = tempfile::tempdir().unwrap();
        let source = Harness::new();
        let app = set_up(&source);
        let sign_in = SettingsItem::SignIn {
            channel: 0,
            network: Network::InstagramReels,
        };
        let path = export(&app, &items(&app.settings_to_export()), dir.path());

        let target = Harness::new();
        let mut other = target.start();
        let file = open(&other, &path);
        let choices = other.settings_to_import(&file);
        let line = choices
            .iter()
            .find(|choice| choice.item == sign_in)
            .unwrap();
        assert!(!line.ticked, "sign-ins are never ticked by default");

        // Without its channel, the sign-in does not come.
        let outcome = other.import_settings(&file, &[sign_in]);
        assert!(outcome.brought.is_empty());
        assert!(sign_in_of(&other).is_none());

        let outcome = other.import_settings(&file, &[SettingsItem::Channel(0), sign_in]);
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        let (connection, tokens) = sign_in_of(&other).unwrap();
        assert_eq!(tokens.access_token(), "access-token-0001");
        assert_eq!(connection.identity.name, "@arquivos");
        assert_eq!(connection.owner, other.profile().id);
        // Only what was ticked came: the channel's persona did not.
        assert!(
            !other
                .personas()
                .unwrap()
                .iter()
                .any(|persona| persona.details.same_name("Narradora"))
        );
    }

    #[test]
    fn bringing_in_again_replaces_by_name_and_adds_nothing_twice() {
        let dir = tempfile::tempdir().unwrap();
        let source = Harness::new();
        let app = set_up(&source);
        let path = export(&app, &ticked(&app.settings_to_export()), dir.path());
        let target = Harness::new();
        let mut other = target.start();
        let file = open(&other, &path);
        let all = ticked(&other.settings_to_import(&file));
        other.import_settings(&file, &all);
        let channels = other.channels().unwrap().len();
        let personas = other.personas().unwrap().len();

        let choices = other.settings_to_import(&file);
        assert!(
            choices
                .iter()
                .all(|choice| choice.effect == Some(ItemEffect::Replaces))
        );
        other.import_settings(&file, &all);
        assert_eq!(other.channels().unwrap().len(), channels);
        assert_eq!(other.personas().unwrap().len(), personas);
    }

    #[test]
    fn nothing_ticked_writes_no_file() {
        let h = Harness::new();
        let app = h.start();
        let error = app
            .export_settings(&[], password(), PathBuf::from("x"))
            .err()
            .unwrap();
        assert_eq!(error.message(), Text::SettingsFileNothingTicked);
    }

    #[test]
    fn a_wrong_password_says_so_and_brings_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = set_up(&h);
        let path = export(&app, &[SettingsItem::Preferences], dir.path());
        let error = app
            .open_settings_file(path.clone(), SettingsPassword::typed("wrong horse!"))
            .run()
            .err()
            .unwrap();
        assert_eq!(error.message(), Text::SettingsFileWrongPassword);
        assert_eq!(
            inspect_settings_file(&path).unwrap().bardo_version,
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn values_a_newer_bardo_wrote_are_listed_not_brought() {
        let json = serde_json::json!({
            "keys": [{"provider": "claude", "key": "sk-ant-api03-from-a-file-0001"}],
            "channels": [{
                "name": "Do Futuro",
                "language": "tlh",
                "country": "US",
            }],
            "playlists": [],
        });
        let bytes = seal_cheaply(json.to_string().as_bytes(), &password());
        let h = Harness::new();
        let mut app = h.start();
        let file = open_bytes(&bytes, &password(), PathBuf::from("x")).unwrap();
        let choices = app.settings_to_import(&file);
        let future = choices
            .iter()
            .find(|choice| choice.item == SettingsItem::Channel(0))
            .unwrap();
        assert_eq!(future.problem, Some(ItemProblem::NotReadable));
        assert!(!future.ticked);
        assert_eq!(app.settings_not_read(&file), ["playlists"]);
        let outcome = app.import_settings(&file, &items(&choices));
        assert_eq!(outcome.brought.len(), 1, "the key still comes");
        assert!(
            !app.channels()
                .unwrap()
                .iter()
                .any(|channel| channel.details.same_name("Do Futuro"))
        );
    }

    #[test]
    fn a_decision_engine_this_copy_lacks_keeps_its_own() {
        let json = serde_json::json!({
            "preferences": {"layout": "studio", "decision_engine": "retired"},
        });
        let bytes = seal_cheaply(json.to_string().as_bytes(), &password());
        let h = Harness::new();
        let mut app = h.start();
        let file = open_bytes(&bytes, &password(), PathBuf::from("x")).unwrap();
        let choices = app.settings_to_import(&file);
        let outcome = app.import_settings(&file, &items(&choices));
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(app.ui_layout(), LayoutId::Studio);
        assert_eq!(app.decision_engine(), DecisionEngineId::Jev);
    }

    #[test]
    fn a_file_name_says_when_it_was_made() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_257_000);
        assert_eq!(
            settings_file_name(at),
            "bardo-settings-20261017-171000.bardo-settings"
        );
    }

    #[test]
    fn unknown_parts_and_fields_are_kept_apart_not_failed() {
        let json = br#"{
            "keys": [{"provider": "claude", "key": "sk-ant-0123456789", "added_later": 1}],
            "playlists": [{"name": "Lo-fi"}]
        }"#;
        let bytes = seal_cheaply(json, &password());
        let file = open_bytes(&bytes, &password(), PathBuf::from("x")).unwrap();
        assert_eq!(file.contents.keys.len(), 1);
        assert_eq!(
            file.contents.unknown.keys().collect::<Vec<_>>(),
            ["playlists"]
        );
    }

    #[test]
    fn contents_that_are_not_bardos_read_as_damage() {
        let bytes = seal_cheaply(b"not json", &password());
        assert!(matches!(
            open_bytes(&bytes, &password(), PathBuf::from("x")),
            Err(SettingsTransferError::File(
                SettingsFileError::WrongPasswordOrDamaged
            ))
        ));
    }

    #[test]
    fn writing_leaves_no_half_file() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("settings.bardo-settings");
        write_file(&to, b"one").unwrap();
        write_file(&to, b"two").unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(write_file(&dir.path().join("missing/x"), b"x").is_err());
    }
}
