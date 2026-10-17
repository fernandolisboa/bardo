# ADR-0014: A copy built from source keeps its own data; settings travel in a password-protected file

- Status: Accepted
- Date: 2026-10-17
- Amends: ADR-0009 (the installer asks for a settings file on a first install), ADR-0010 (a settings file sits beside backups in Settings › Backup)

## Context

A copy of Bardo built from source and the installed release used the same folders (`%APPDATA%\Bardo`, `%LOCALAPPDATA%\Bardo`) and the same background agent task. Source is almost always newer than the latest release, so testing a change from source migrated the one database the installed Bardo opens, and an older installed Bardo then opened a database with migrations it did not know. Refusing to open such a database would leave the installed Bardo unusable after any test from source.

The user also wants to set up keys, app credentials, preferences, personas and channels once, in either copy, and take only part of them to the other. A backup (ADR-0010) carries the whole database and never a secret; copying secrets into a plain file would expose them.

## Decision

1. **Two data homes.** A copy runs from its own data home, chosen once at start, before anything opens the data: `Bardo` for an installed copy, `Bardo Dev` for a copy built from source (no third-party notices beside the program, or not on Windows; `bardo_app::this_data_home`). The database, project folders, voice samples, logs, backups, updates and `setup-choices.toml` follow it, and the background agent task is named after it (`Bardo Dev publishing agent (user)`). Windows Credential Manager entries already carry the profile's id (`Bardo/{profile}/…`), and each data home makes its own profile, so secrets stay apart without another prefix. A copy built from source shows **Dev** in its title bar and says so in Settings › About.
2. **A settings file.** Settings › Backup › Settings file exports the ticked settings into one `.bardo-settings` file and imports one after the user ticks what comes in. It can carry provider API keys, app credentials, preferences (language, theme, layout, tour offers, weekly backup, update checks, metrics sync, cut suggestion floor), budgets and changed prices, personas (as persona packages) and channels with their network accounts. A network account's sign-in can travel too, only with its channel, and is unticked by default: networks that rotate refresh tokens (X) sign one copy out once the other renews.
3. **The file is encrypted with a password.** The file is a JSON envelope (`format: "bardo-settings"`, `version: 1`, the Bardo version and time that made it, the key derivation and cipher parameters) around the sealed contents. The key comes from the password with Argon2id (64 MiB, 3 iterations, 1 lane, a random 16-byte salt); the contents are sealed with XChaCha20-Poly1305 (a random 24-byte nonce), with the envelope's header as associated data, so nothing in it can be changed unnoticed. A password has at least 10 characters and is typed twice on export. A wrong password and a damaged file give the same answer. Parameters beyond 1 GiB, 16 iterations or 8 lanes are refused before any work, so a crafted file cannot exhaust the machine. Secrets are wiped from memory once used.
4. **The contents are an interchange format, not a database copy.** Every part is optional and read on its own. A newer Bardo's file of the same version opens: parts and values this Bardo does not know are listed and left out, never failing the whole file. A higher `version` asks the user to update Bardo.
5. **Importing adds what is new and replaces by name.** A key or app credential of the same network, and a persona or channel of the same name, take the file's details; a persona or channel keeps its id, so its projects and the channels that use it stay. Each item is saved on its own and one that fails is listed while the others come in. Nothing unticked changes. Imported YouTube items wait for the privacy policy like any others (ADR-0012).
6. **Offered at the right moments.** The first start of a new copy shows a bar offering to import a settings file. The installer's first install has an optional page to pick one; it writes the path to `setup-choices.toml`, and Bardo asks for its password at the next start. The installer never handles the password.

## Consequences

- Testing from source never touches the installed Bardo's data, and the installed Bardo never opens a database a newer build migrated. Data a source build made before this change stays in the `Bardo` folders, where an installed Bardo finds it.
- The two copies' settings drift unless the user exports again; a settings file is a snapshot, not a sync.
- Forgetting a file's password loses the file: there is no recovery by design.
- Anyone holding both a settings file and its password can read the keys in it; the screen and the guide tell the user to keep them apart.
