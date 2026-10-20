# Windows end to end

The checks of the Windows test script (issues #51 and #86) that a machine can
make, run by `.github/workflows/windows-e2e.yml` on a clean GitHub Actions
Windows runner against the Bardo each commit builds (issue #166).

No real provider key, account or paid call is used: the keys and app
credentials are made-up values, and only Bardo's own local state is checked.
The checks the runner cannot make (real accounts, uploads, GPUs, display
scaling, a person's judgment) stay in the manual script.

## The runner

`windows-latest`: Windows Server with no GPU (Bardo draws through WARP), one
1024 × 768 screen at 100%, an interactive desktop, Task Scheduler and
Credential Manager. The user is an administrator; Bardo installs per user
regardless.

## Jobs

| Job | What it does |
| --- | --- |
| Build the installer | `cargo xtask package`, plus `bardo-e2e` (below). The artifact holds the installer, the staged `bardo.exe` and `bardo-e2e.exe`. |
| Installer (fresh) | Walks the wizard on a clean machine, uninstalls and reinstalls. |
| Installer (upgrade) | Installs the latest published release, then this build over it, silently and through the wizard. |
| App | Installs this build and drives it through `bardo-e2e`. |

Each job writes `results.json` (one row per check), screenshots and Bardo's
logs to its artifact, and a table to the run's summary. A failed check fails
the job; the remaining checks still run.

## How the checks reach Bardo

- **From outside**: files and folders, the uninstall entry in the registry,
  `bardo.exe` processes, the agent's task (`schtasks /XML`), Credential
  Manager (`cmdkey /list`), zips Bardo writes, screenshots of the screen. The
  installer's wizard is read and clicked through UI Automation.
- **Through the app state**: `crates/e2e` builds `bardo-e2e`, which starts
  Bardo the way its window does (same data home, profile, vault, logs and
  agent task) and calls the same `app` functions the screens call, printing
  JSON. A copy beside the installed `bardo.exe` acts as the installed Bardo;
  a copy beside a `bardo.exe` outside the installed layout acts as a copy
  built from source (Bardo Dev). The agent task it registers runs the
  `bardo.exe` beside it. It is never shipped.

| `bardo-e2e` command | What it does |
| --- | --- |
| `state` | Profile, data home, keys and app credentials (saved, hint), agent, settings offer, last restore, update banner. |
| `save-keys`, `save-app-credentials` | Saves made-up keys and app credentials for every provider and network. |
| `seed-channel` | Adds a channel with an Instagram account. |
| `export-settings FILE PASSWORD`, `import-settings FILE PASSWORD` | Settings transfer, everything ticked. |
| `diagnostics ZIP` | Saves the diagnostics zip. |
| `agent on\|off\|start` | The Settings switch and Start now. |
| `backup ZIP [media]`, `restore ZIP` | Backup and restore, as from Settings › Backup. |
| `window-lock` | Whether this start would hold the data, as Bardo's window does. |

## Checks by script step

| Step | Job | Checked |
| --- | --- | --- |
| S1.01 | Installer (fresh) | The wizard asks the agent question, no Bardo opens before Finish, Launch Bardo opens it; the agent's task; an unticked install leaves `background_agent = false`. |
| S1.08 | Installer (upgrade) | Installing over a running Bardo and agent closes them, replaces the files, keeps the agent setting and does not ask again. |
| S1.10 | Installer (upgrade) | The wizard over the same version: no Import settings page, no agent question, no relaunch. |
| S2.05 | Installer | Bardo draws its window without a GPU (screenshot not blank). |
| S2.08, S2.09, S2.10 | App | A copy from source keeps its data as Bardo Dev, with its own profile; offers to bring settings in once; reads as built from source with no update banner; leaves the installed `bardo.db` untouched. |
| S3.06, S3.08 | App | One Credential Manager entry per key, hints show only the end, nothing secret in the logs; the privacy policy stays accepted. |
| S3.10, S3.11 | App | Export from the source copy into a folder with accents; no secret readable in the file; wrong password refused; the right one lists replaces/adds and brings everything in. |
| S5.09, S5.15, S5.22, S5.29 | App | App credentials are generic `Bardo/<profile>/app/<network>` entries; the secret shows only its end. |
| S5.34 | App | `bardo.db` holds no key or secret. |
| S10.36, S10.37, S10.38, S10.43 | App | The agent's task (name, command, trigger, principal); on and off from Settings; an agent ended by hand reads not running and Start now restarts it; the source copy's agent has its own task. |
| S13.01–S13.04 | App | Diagnostics: logs folder, zip contents, Windows build, decision engine, nothing private in it, overwrite, unwritable folder. |
| S13.06, S13.09–S13.12 | App | Backup with media and the agent running; the data lock; restore at the next start; rollback with a project folder in use; restore without the keys lists what to set up again. |
| S13.13 | Installer (fresh) | Uninstall refused with the window open; with the agent running it removes the task and program but keeps data; reinstall finds the projects; an install never opened leaves no database. |
| S13.14 | Installer (fresh) | The Import settings page on a first install; `setup-choices.toml` keeps the answers in UTF-8. |

## Running a script by hand

On a Windows machine with PowerShell 7, from the repository root, with a
built installer:

```powershell
./e2e/windows/installer.ps1 -Scenario fresh -Setup <Bardo-Setup.exe> -Program target/package/Bardo/bardo.exe -Out e2e-out
./e2e/windows/app.ps1 -Setup <Bardo-Setup.exe> -Harness target/release/bardo-e2e.exe -Out e2e-out
```

The scripts install, uninstall and change Bardo's data, its agent task and
Credential Manager entries for the signed-in user: run them only on a
throwaway machine.
