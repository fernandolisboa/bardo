# Bardo

Native Windows desktop app to create and operate faceless ("dark") channels across YouTube, TikTok, Instagram Reels, X and Kick.

AI-assisted, not automatic: AI speeds up research, scripting, media generation and cut suggestions; the user decides, adjusts and finalizes. Single-user and offline-first: everything runs on the user's machine and secrets stay local.

- Domain vocabulary: [`docs/CONTEXT.md`](docs/CONTEXT.md)
- Architecture decisions: [`docs/adr/`](docs/adr/)
- Specs: [`docs/spec/`](docs/spec/)
- PRD: [`docs/prd/`](docs/prd/)
- Design handoffs: [`docs/design/`](docs/design/)
- Spikes: [`docs/spikes/`](docs/spikes/)

## Development

Requires Windows 10/11 and the stable Rust toolchain (MSVC).

```sh
cargo xtask fetch-ffmpeg  # once: the pinned ffmpeg build into .ffmpeg/ (ADR-0007)
cargo run                 # opens the app (crate bardo-ui)
cargo test --workspace
cargo xtask lint-deps     # fails if any crate other than bardo-ui depends on GPUI
```

Workspace layout follows [ADR-0001](docs/adr/0001-architecture-and-stack.md): `crates/{domain,media,ai,publish,storage,app,ui}`, plus `xtask` for repository checks. The local profile and its settings live in `%APPDATA%\Bardo\bardo.db`.

UI strings live in `crates/app/locales/{en-US,pt-BR}.toml`; tests fail if a key is missing from either file.
