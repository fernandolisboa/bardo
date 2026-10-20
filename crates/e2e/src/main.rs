//! `bardo-e2e`: drives Bardo through its app state for the Windows
//! end-to-end jobs (issue #166), the way the window does, against the real
//! platform: the user's folders, Credential Manager and Task Scheduler.
//!
//! It runs from a folder holding a `bardo.exe`: beside the installed one it
//! is the installed copy (its folders, its ffmpeg); beside a copy outside
//! the installed layout it is a copy built from source (`Bardo Dev`). The
//! agent's task it sets up runs that `bardo.exe`, as the window's would.
//!
//! Each command does one thing a person does in Bardo and prints what Bardo
//! shows as JSON; the job's script checks the rest from outside.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use bardo_app::{
    AgentStatus, Bardo, ItemEffect, KeyState, Providers, Repositories, SettingsPassword,
};
use bardo_domain::{
    AgentTask, AgentTaskError, ChannelDraft, ContentLanguage, Country, JobId, Network,
    NetworkAccountDraft, Provider,
};
use bardo_storage::{DataPaths, Database};
use serde_json::{Value, json};

const USAGE: &str = "usage: bardo-e2e <state | save-keys | save-app-credentials | seed-channel \
                     | export-settings <file> <password> | import-settings <file> <password> \
                     | diagnostics <zip> | agent <on|off|start> | backup <zip> [media] \
                     | restore <zip> | window-lock>";

/// How long a job may take before the command gives up on it.
const JOB_PATIENCE: Duration = Duration::from_secs(300);

/// The channel and account names `seed-channel` makes, which a diagnostic
/// export must never show.
const CHANNEL_NAME: &str = "Canal Único de Teste";
const ACCOUNT_HANDLE: &str = "bardo_e2e_handle";

fn main() -> anyhow::Result<()> {
    bardo_storage::use_data_home(bardo_app::this_data_home());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        bail!("{USAGE}");
    };
    if command == "window-lock" {
        return window_lock();
    }
    let mut bardo = start()?;
    let out = match (command.as_str(), &args[1..]) {
        ("state", []) => state(&bardo)?,
        ("save-keys", []) => save_keys(&mut bardo)?,
        ("save-app-credentials", []) => save_app_credentials(&mut bardo)?,
        ("seed-channel", []) => seed_channel(&bardo)?,
        ("export-settings", [file, password]) => export_settings(&bardo, file, password)?,
        ("import-settings", [file, password]) => import_settings(&mut bardo, file, password)?,
        ("diagnostics", [zip]) => diagnostics(&bardo, zip)?,
        ("agent", [step]) => agent(&mut bardo, step)?,
        ("backup", [zip]) => backup(&bardo, zip, false)?,
        ("backup", [zip, media]) if media == "media" => backup(&bardo, zip, true)?,
        ("restore", [zip]) => restore(&bardo, zip)?,
        _ => bail!("{USAGE}"),
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

/// Opens Bardo as the window does, logging where it logs.
fn start() -> anyhow::Result<Bardo> {
    let paths = DataPaths::local_default()?;
    let db_path = bardo_storage::default_database_path()?;
    let db = Database::open(&db_path)
        .with_context(|| format!("opening the database at {}", db_path.display()))?;
    let files = bardo_storage::LocalProjectFiles::new(bardo_storage::default_projects_dir()?);
    let exports = bardo_storage::LocalExportFiles::new(bardo_storage::default_exports_dir());
    let voice_samples =
        bardo_storage::LocalVoiceSamples::new(bardo_storage::default_voice_samples_dir()?);
    let mut repositories = Repositories::local(
        db,
        bardo_storage::platform_secret_store()?,
        bardo_storage::platform_connection_secrets()?,
        Box::new(files),
        Box::new(exports),
        Box::new(voice_samples),
        paths,
    );
    repositories.agent_task = Arc::new(BesideBardo(repositories.agent_task.clone()));
    let bardo = Bardo::start(repositories, Providers::live(), Some("en-US"))?;
    if let Some(log) = bardo_app::logging::default_log_path() {
        let _ = bardo_app::logging::init(&log, bardo.redactor());
    }
    Ok(bardo)
}

/// The agent's task, set up to run the `bardo.exe` beside this program,
/// as Bardo's window sets it up to run itself.
struct BesideBardo(Arc<dyn AgentTask>);

impl AgentTask for BesideBardo {
    fn is_registered(&self) -> Result<bool, AgentTaskError> {
        self.0.is_registered()
    }

    fn register(&self, program: &Path) -> Result<(), AgentTaskError> {
        self.0.register(&bardo_beside(program))
    }

    fn start(&self) -> Result<(), AgentTaskError> {
        self.0.start()
    }

    fn remove(&self) -> Result<(), AgentTaskError> {
        self.0.remove()
    }
}

fn bardo_beside(program: &Path) -> PathBuf {
    program.with_file_name(if cfg!(windows) { "bardo.exe" } else { "bardo" })
}

/// What the screens would show, for the script to read.
fn state(bardo: &Bardo) -> anyhow::Result<Value> {
    let keys: serde_json::Map<String, Value> = bardo
        .provider_keys()
        .into_iter()
        .map(|status| (status.provider.code().to_owned(), key_state(&status.state)))
        .collect();
    let credentials: serde_json::Map<String, Value> = bardo
        .app_credentials()
        .into_iter()
        .map(|status| (status.network.code().to_owned(), key_state(&status.state)))
        .collect();
    let backups = bardo.backup_view();
    let updates = bardo.update_view();
    Ok(json!({
        "profile": bardo.profile().id.to_string(),
        "data_home": bardo_storage::data_home().folder(),
        "privacy_accepted": bardo.privacy_accepted(),
        "agent": agent_status(bardo.background_agent()),
        "keys": keys,
        "app_credentials": credentials,
        "channels": bardo.channels()?.iter().map(|c| c.details.name().to_owned()).collect::<Vec<_>>(),
        // Offered with no file on a copy's first start; with the installer's
        // file after a first install.
        "settings_offer": bardo.settings_offer().map(|offer| json!({ "file": offer.file })),
        "restart_needed": backups.restart_needed,
        "restore_block": backups.restore_block.map(|block| format!("{block:?}")),
        "last_restore": backups.last_restore.map(|outcome| json!({
            "succeeded": outcome.succeeded(),
            "failure": outcome.failure,
        })),
        "set_up_again": backups.set_up_again.iter().map(|item| format!("{item:?}")).collect::<Vec<_>>(),
        "installation": format!("{:?}", updates.installation),
        "update_check_on": updates.check_on,
        "update_banner": updates.banner,
    }))
}

fn key_state(state: &KeyState) -> Value {
    match state {
        KeyState::Saved { hint } => json!({ "saved": true, "hint": hint }),
        other => json!({ "saved": false, "state": format!("{other:?}") }),
    }
}

fn agent_status(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Off => "off",
        AgentStatus::Running => "running",
        AgentStatus::NotRunning => "not-running",
    }
}

/// A made-up key in each provider's shape, distinct per profile so a
/// search for it finds only this run's.
fn fixture_key(provider: Provider, profile: &str) -> String {
    let tail: String = profile
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    match provider {
        Provider::Higgsfield => format!("e2e-key-id-{tail}:e2e-key-secret-{tail}"),
        other => format!("e2e-{}-key-{tail}", other.code()),
    }
}

/// Saves a key for every provider that takes one, after accepting the
/// privacy policy YouTube's key waits for. Prints the keys.
fn save_keys(bardo: &mut Bardo) -> anyhow::Result<Value> {
    bardo.accept_privacy_policy()?;
    let profile = bardo.profile().id.to_string();
    let mut saved = serde_json::Map::new();
    for provider in Provider::with_keys() {
        let key = fixture_key(provider, &profile);
        bardo
            .save_provider_key(provider, &key)
            .map_err(|error| anyhow!("saving the {} key: {error:?}", provider.code()))?;
        saved.insert(provider.code().to_owned(), json!(key));
    }
    Ok(json!({ "profile": profile, "keys": saved }))
}

/// Made-up app credentials in each network's shape.
fn fixture_credentials(network: Network, profile: &str) -> (String, String) {
    let tail: String = profile
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(12)
        .collect();
    let digits: String = tail.bytes().map(|b| char::from(b'0' + b % 10)).collect();
    let id = match network {
        Network::YouTube => format!("e2e{tail}.apps.googleusercontent.com"),
        Network::InstagramReels => format!("1{digits}"),
        Network::TikTok => format!("sbawe2e{tail}"),
        _ => format!("e2e-client-{tail}"),
    };
    (id, format!("e2e-secret-{}-{tail}", network.code()))
}

/// Saves app credentials for every network Bardo signs in to. Prints them.
fn save_app_credentials(bardo: &mut Bardo) -> anyhow::Result<Value> {
    if !bardo.privacy_accepted() {
        bardo.accept_privacy_policy()?;
    }
    let profile = bardo.profile().id.to_string();
    let mut saved = serde_json::Map::new();
    for network in Network::sign_in_networks() {
        let (id, secret) = fixture_credentials(network, &profile);
        bardo
            .save_app_credentials(network, &id, &secret)
            .map_err(|error| anyhow!("saving {} app credentials: {error:?}", network.code()))?;
        saved.insert(
            network.code().to_owned(),
            json!({ "id": id, "secret": secret }),
        );
    }
    Ok(json!({ "profile": profile, "app_credentials": saved }))
}

/// A channel with an account whose handle a diagnostic must hide.
fn seed_channel(bardo: &Bardo) -> anyhow::Result<Value> {
    let channel = bardo
        .create_channel(ChannelDraft {
            name: CHANNEL_NAME.into(),
            niche: "space history".into(),
            themes: vec!["Apollo".into()],
            aesthetic_notes: "archival".into(),
            language: ContentLanguage::English,
            country: Country::UnitedStates,
            default_persona: None,
            clip_model: None,
            caption_style: Default::default(),
            transition: Default::default(),
        })
        .map_err(|error| anyhow!("creating the channel: {error:?}"))?;
    bardo
        .add_network_account(
            channel.id,
            Network::InstagramReels,
            NetworkAccountDraft {
                handle: ACCOUNT_HANDLE.into(),
                ..NetworkAccountDraft::default()
            },
        )
        .map_err(|error| anyhow!("adding the account: {error:?}"))?;
    Ok(json!({ "channel": CHANNEL_NAME, "handle": ACCOUNT_HANDLE }))
}

/// Exports everything the export list ticks by default.
fn export_settings(bardo: &Bardo, file: &str, password: &str) -> anyhow::Result<Value> {
    let ticked: Vec<_> = bardo
        .settings_to_export()
        .into_iter()
        .filter(|choice| choice.ticked)
        .collect();
    let items: Vec<_> = ticked.iter().map(|choice| choice.item).collect();
    let password = SettingsPassword::choose(password, password)
        .map_err(|error| anyhow!("the password: {error:?}"))?;
    let written = bardo
        .export_settings(&items, password, PathBuf::from(file))
        .and_then(|export| export.run())
        .map_err(|error| anyhow!("exporting: {error:?}"))?;
    Ok(json!({
        "file": written,
        "items": ticked.iter().map(|choice| choice.label.clone()).collect::<Vec<_>>(),
    }))
}

/// Opens the file with `password`; when it opens, lists what it holds and
/// what each would do here, then brings in what the list ticks.
fn import_settings(bardo: &mut Bardo, file: &str, password: &str) -> anyhow::Result<Value> {
    let opened = match bardo
        .open_settings_file(PathBuf::from(file), SettingsPassword::typed(password))
        .run()
    {
        Ok(opened) => opened,
        Err(error) => {
            return Ok(json!({
                "opened": false,
                "message": bardo.text(error.message()).into_owned(),
            }));
        }
    };
    let choices = bardo.settings_to_import(&opened);
    let listed: Vec<Value> = choices
        .iter()
        .map(|choice| {
            json!({
                "label": choice.label,
                "effect": match choice.effect {
                    Some(ItemEffect::Adds) => "adds",
                    Some(ItemEffect::Replaces) => "replaces",
                    None => "none",
                },
                "ticked": choice.ticked,
            })
        })
        .collect();
    let ticked: Vec<_> = choices
        .iter()
        .filter(|choice| choice.ticked)
        .map(|choice| choice.item)
        .collect();
    let outcome = bardo.import_settings(&opened, &ticked);
    Ok(json!({
        "opened": true,
        "listed": listed,
        "brought": outcome.brought.len(),
        "failed": outcome.failed.iter().map(|(choice, _)| choice.label.clone()).collect::<Vec<_>>(),
        "profile": bardo.profile().id.to_string(),
    }))
}

/// Writes a diagnostic export to `zip`, as Settings › About does.
fn diagnostics(bardo: &Bardo, zip: &str) -> anyhow::Result<Value> {
    let logs = bardo_app::logging::logs_dir().context("no logs folder")?;
    let display = bardo_app::DisplayInfo {
        gpu: None,
        renderer: "DirectX 11".into(),
        theme: bardo.ui_theme().resolve(bardo_domain::ThemeMode::Light),
    };
    let export = bardo.diagnostics(&logs, &display);
    let entries = export.entries();
    let written = export.write_to(Path::new(zip));
    Ok(json!({
        "entries": entries,
        "written": written.is_ok(),
        "error": written.err().map(|error| error.to_string()),
        "logs": logs,
    }))
}

/// Settings › Publishing: on, off, or Start now.
fn agent(bardo: &mut Bardo, step: &str) -> anyhow::Result<Value> {
    let work = match step {
        "on" => bardo.set_background_agent(true)?,
        "off" => bardo.set_background_agent(false)?,
        "start" => bardo.start_background_agent(),
        _ => bail!("{USAGE}"),
    };
    let result = work.run();
    if result.is_err() && step == "on" {
        bardo.background_agent_not_set_up()?;
    }
    Ok(json!({
        "error": result.err().map(|error| error.to_string()),
        "agent": agent_status(bardo.background_agent()),
    }))
}

/// Settings › Back up…, waiting for the job.
fn backup(bardo: &Bardo, zip: &str, media: bool) -> anyhow::Result<Value> {
    let job = bardo
        .start_backup(PathBuf::from(zip), media)
        .map_err(|error| anyhow!("starting the backup: {error:?}"))?;
    Ok(json!({ "job": wait_for(bardo, job)? }))
}

/// Settings › Restore…: reviews the backup, then restores it up to the
/// restart that puts it in place.
fn restore(bardo: &Bardo, zip: &str) -> anyhow::Result<Value> {
    let review = bardo
        .backup_check(PathBuf::from(zip))
        .run()
        .map_err(|error| anyhow!("reviewing the backup: {error:?}"))?;
    let job = match bardo.start_restore(&review) {
        Ok(job) => job,
        Err(error) => return Ok(json!({ "started": false, "refused": format!("{error:?}") })),
    };
    let finished = wait_for(bardo, job)?;
    Ok(json!({
        "started": true,
        "job": finished,
        "restart_needed": bardo.backup_view().restart_needed,
    }))
}

/// Waits for `job` to end; how it ended.
fn wait_for(bardo: &Bardo, id: JobId) -> anyhow::Result<Value> {
    let deadline = Instant::now() + JOB_PATIENCE;
    loop {
        let job = bardo
            .jobs()
            .into_iter()
            .find(|job| job.id() == id)
            .context("the job went missing")?;
        if !job.state().is_active() {
            return Ok(json!({
                "state": job.state().code(),
                "failure": job.failure().map(|failure| format!("{failure:?}")),
            }));
        }
        if Instant::now() > deadline {
            bail!("the job did not end within {JOB_PATIENCE:?}");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Whether this would be the only window: Bardo's window takes the same
/// lock at start.
fn window_lock() -> anyhow::Result<()> {
    let paths = DataPaths::local_default()?;
    let sole = bardo_storage::lock_window(&paths, Duration::ZERO)?.is_some();
    println!("{}", json!({ "sole": sole }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "5f0c2a1e-8d3b-4c7a-9e21-0a1b2c3d4e5f";

    #[test]
    fn every_fixture_key_is_one_bardo_accepts() {
        for provider in Provider::with_keys() {
            let key = fixture_key(provider, PROFILE);
            bardo_domain::ApiKey::parse(provider, &key)
                .unwrap_or_else(|error| panic!("{}: {error:?}", provider.code()));
        }
    }

    #[test]
    fn every_fixture_credential_is_one_bardo_accepts() {
        for network in Network::sign_in_networks() {
            let (id, secret) = fixture_credentials(network, PROFILE);
            bardo_domain::AppCredentials::parse(network, &id, &secret)
                .unwrap_or_else(|error| panic!("{}: {error:?}", network.code()));
        }
    }

    #[test]
    fn fixture_secrets_differ_per_profile() {
        let other = "9a8b7c6d-0000-4000-8000-000000000000";
        assert_ne!(
            fixture_key(Provider::Claude, PROFILE),
            fixture_key(Provider::Claude, other)
        );
    }

    #[test]
    fn the_task_runs_the_bardo_beside_this_program() {
        let program = Path::new("dir").join("bardo-e2e");
        assert_eq!(
            bardo_beside(&program).file_stem().unwrap(),
            "bardo",
            "the agent is Bardo itself"
        );
        assert_eq!(bardo_beside(&program).parent(), program.parent());
    }
}
