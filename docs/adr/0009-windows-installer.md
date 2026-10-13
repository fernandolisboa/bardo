# ADR-0009: Windows installer and releases

- Status: Accepted
- Date: 2026-10-13
- Amends: ADR-0006 (the installer's background agent offer), ADR-0007 (shipping and mirroring ffmpeg)

## Context

Until now Bardo ran only from source: a Rust toolchain, `cargo xtask fetch-ffmpeg` and `cargo run`. ADR-0006 left the background agent's offer and ADR-0007 left FFmpeg's distribution to "the installer slice" (issue #135). Bardo is per user: its data lives in `%APPDATA%\Bardo`, its keys in the user's Credential Manager, and its agent is a Task Scheduler task of the user that needs no administrator.

## Decision

1. **Inno Setup 6**, compiled by `cargo xtask package` from `installer/bardo.iss`, one `Bardo-Setup-<version>-x64.exe` with English and Brazilian Portuguese. Chosen over WiX/MSI, whose per-user installs, custom question pages and bilingual dialogs take far more code for the same result, and over MSIX, which needs a signed package to install at all.
2. **Per user, no administrator** (`PrivilegesRequired=lowest`): `%LOCALAPPDATA%\Programs\Bardo`, Start menu and optional desktop shortcut, uninstall entry under HKCU. Nothing needs elevation, matching the per-user agent.
3. **The installed layout** is the one `bardo-media` already searches: `bardo.exe`, `ffmpeg\` with `ffmpeg.exe`, `ffprobe.exe` and the shared DLLs unmodified and unrenamed, FFmpeg's `LICENSE.txt` and a `SOURCE.txt` naming the build, its SHA-256 and where its source is, and `THIRD-PARTY-NOTICES.html` made by `cargo about` (`installer/about.toml`, `installer/about.hbs`) for the crates `bardo.exe` links on Windows. A crate under a license not listed in `about.toml` fails the package until it is reviewed.
4. **The background agent question** (ADR-0006's copy) is a task on the installer's page, asked on the first install only and unticked by default. The installer writes the answer to `%APPDATA%\Bardo\setup-choices.toml`; Bardo reads it once at its next start (`bardo_storage::take_setup_choices`), saves it to the profile and deletes the file, and its usual upkeep registers the task. The installer never calls `schtasks`, so the task definition stays in one place.
5. **Upgrades** install over the same app id. `CloseApplications=force` closes a running Bardo and its windowless agent before files are replaced; the agent's jobs go back through their leases (ADR-0006) and the agent starts again at the next sign-in or 15-minute check. `ffmpeg\` is emptied first, so no old DLL outlives an FFmpeg update.
6. **Uninstall** refuses while Bardo's window is open (asks to close it), then runs `bardo --uninstall`: the agent is turned off, its task removed, and a running agent has left before files go. User data, exports and Credential Manager entries stay. On a machine where Bardo never opened, `--uninstall` creates no database.
7. **Releases.** `.github/workflows/release.yml` builds the installer on `windows-latest` (which carries Inno Setup 6): a `v*` tag that matches the workspace version drafts a GitHub Release with the installer and its SHA-256; a manual run keeps it as a run artifact for test builds; pull requests that touch the packaging build it too. Publishing a draft is a person's call.
8. **FFmpeg mirror.** `.github/workflows/ffmpeg-mirror.yml` copies both pinned archives and the FFmpeg source at the build's commit to the `ffmpeg-<version>` release of this repository (`cargo xtask mirror-ffmpeg`) when the pin changes on main; `fetch-ffmpeg` falls back to it when the upstream link is gone, and `SOURCE.txt` and Settings › About point at its source copy.
9. **Settings › About** shows Bardo's version, the FFmpeg credit with its license and a link to the source, and opens the notices page.
10. **Unsigned for now.** SmartScreen warns on the first run; the guide's "Installing Bardo" page says how to go on. `bardo.iss` and the release workflow keep one place for a signing step.

## Consequences

- Testing on Windows no longer needs a toolchain: a manual run of the Release workflow gives an installer.
- A new FFmpeg version needs a new mirror release, which the mirror workflow makes when the pin lands on main.
- A new dependency under an unlisted license fails the package (and the pull request that touches packaging), so licenses are reviewed when they come in, not at release time.
- Without signing, some users will stop at SmartScreen. Signing (for example Azure Trusted Signing or an OV certificate) is a later decision with a running cost.
- There is no auto-update: users run the newer installer over the old one.
