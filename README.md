# Bardo

Native Windows desktop app to create and operate faceless ("dark") channels across YouTube, TikTok, Instagram Reels, X and Kick.

AI-assisted, not automatic: AI speeds up research, scripting, media generation and cut suggestions; the user decides, adjusts and finalizes. Single-user and offline-first: everything runs on the user's machine and secrets stay local.

- User guide: [fernandolisboa.github.io/bardo](https://fernandolisboa.github.io/bardo/) (English and Portuguese; source in [`docs/guide/`](docs/guide/))
- Domain vocabulary: [`docs/CONTEXT.md`](docs/CONTEXT.md)
- Architecture decisions: [`docs/adr/`](docs/adr/)
- Specs: [`docs/spec/`](docs/spec/)
- PRD: [`docs/prd/`](docs/prd/)
- Design handoffs: [`docs/design/`](docs/design/)
- Brand (mark, wordmark, app icon): [`assets/brand/`](assets/brand/)
- Spikes: [`docs/spikes/`](docs/spikes/)

## Installing

Download `Bardo-Setup-<version>-x64.exe` from [Releases](https://github.com/fernandolisboa/bardo/releases) and run it: it installs for your Windows user, with no administrator rights, and brings FFmpeg with it. See the guide's [Installing Bardo](docs/guide/en-US/install.md) and [ADR-0009](docs/adr/0009-windows-installer.md). What changed in each version is in [`CHANGELOG.md`](CHANGELOG.md).

## Development

Requires Windows 10/11 and the stable Rust toolchain (MSVC).

```sh
cargo xtask fetch-ffmpeg  # once: the pinned ffmpeg build into .ffmpeg/ (ADR-0007)
cargo run                 # opens the app (crate bardo-ui)
cargo test --workspace
cargo xtask lint-deps     # fails if any crate other than bardo-ui depends on GPUI
cargo xtask package       # the installed layout in target/package/Bardo and, with
                          # Inno Setup 6 and cargo-about installed, the installer
```

`cargo xtask package` needs [Inno Setup 6](https://jrsoftware.org/isdl.php) and `cargo install cargo-about --locked --features cli`. The Release workflow runs it on Windows: a manual run gives an installer to test, a `v*` tag drafts a release.

Workspace layout follows [ADR-0001](docs/adr/0001-architecture-and-stack.md): `crates/{domain,media,ai,publish,storage,app,ui}`, plus `xtask` for repository checks. The local profile and its settings live in `%APPDATA%\Bardo\bardo.db`.

UI strings live in `crates/app/locales/{en-US,pt-BR}.toml`; tests fail if a key is missing from either file.
