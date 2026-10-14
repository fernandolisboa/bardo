# ADR-0011: Updating from GitHub Releases

- Status: Accepted
- Date: 2026-10-14
- Amends: ADR-0009 (the installer also runs silently for an update, and opens Bardo again)

## Context

Until now a user updated by downloading the next installer from the releases page and running it over the installed copy (ADR-0009). Issue #144 brings this into the app. Releases are published on GitHub by the release workflow: a `v*` tag builds `Bardo-Setup-<version>-x64.exe` and `Bardo-Setup-<version>-x64.exe.sha256` (`cargo xtask package`) and attaches both to the release. The installer is not code-signed (ADR-0009), so the only check Bardo can make on a file it downloads is against what the release publishes. The same repository holds `ffmpeg-*` releases (ADR-0007), which are not Bardo versions.

## Decision

1. **The feed is GitHub's `releases/latest`** for `fernandolisboa/bardo`, asked over HTTPS without a token. It answers with the newest published release that is neither a draft nor a pre-release, so the `ffmpeg-*` mirrors (pre-releases) never show, and 404 while nothing is published. Its tag is read as a semantic version (`v` prefix optional); a newer one than the running build is an update. Without a token GitHub allows 60 requests an hour from an address; Bardo asks at most once at start and once a day, and a rate-limited answer says when to try again.
2. **When it checks:** Bardo's window checks shortly after it opens and every 24 hours while it stays open, unless the user turns *Check for updates automatically* off in Settings › About (on by default; a column on the profile). *Check now* always checks. The background agent never checks. A failed check keeps what an earlier one found.
3. **What the user sees:** a bar over the screens names the newer version, with *See what's new* (Settings › About) and *Later*, which hides it for that version until Bardo opens again. Settings › About shows the release's notes, its page, and the download and install steps. Nothing is downloaded or installed until the user asks.
4. **The download is a job** (`update_download`), with progress and cancel like any other. It reads the release's `.sha256` file first, which must name the installer, and, when GitHub's own asset digest is present, the two must agree. It streams the installer to `%LOCALAPPDATA%\Bardo\updates\<name>.part` while hashing it, then requires the exact announced size and the published SHA-256 before renaming it to its final name. A mismatch deletes the file and fails the job (`checksum`); a cut or cancelled download deletes the part. Files come only from `https://github.com/fernandolisboa/bardo/releases/download/<tag>/<file>`, redirects stay on HTTPS, and the file may not exceed 1 GiB.
5. **Installing is the user's call after a review.** It waits while jobs run in this window, during a restore, and while another window is open (the installer would close it). Bardo hashes the file once more, starts it as its own process with `/SILENT /SUPPRESSMSGBOXES /NORESTART /RELAUNCH=1`, and quits. The installer closes the background agent (`CloseApplications=force`, whose jobs go back through their leases), replaces the files with only its progress window, and, for `/RELAUNCH=1` in a silent run, opens Bardo again. A silent install run by hand leaves Bardo closed.
6. **How Bardo was installed decides how it updates.** A copy in the Microsoft Store's `WindowsApps` folder never checks (the Store updates it, #146). The installed layout (the third-party notices next to `bardo.exe`, on Windows) downloads and installs. Any other copy, a build from source, says when a release is out and links to it.
7. **Tidying:** at start, Bardo's window (alone) deletes installers of its own version or older and part files no download is writing.

## Consequences

- No new secret, account or service: the check is one anonymous HTTPS request a day, and it sends nothing about the user.
- The SHA-256 protects against a corrupted or truncated download and a file changed on disk; it does not protect against a compromised release, which would publish a matching checksum. Signing the installer would; when it is signed (ADR-0009), Bardo should also verify the signature (`WinVerifyTrust`) before running it.
- A file Bardo downloads carries no Mark of the Web, so Windows SmartScreen does not show *Windows protected your PC* for an update the way it does for the first, browser-downloaded installer. Smart App Control, where it is on, still judges the unsigned installer.
- The installer is per user (`PrivilegesRequired=lowest`), so an update asks for no administrator rights.
- The relaunch lives in the new version's installer: updating from a version whose installer predates it still works, because it is the newer installer that runs.
- Downgrades are not offered: only a newer version counts as an update.
