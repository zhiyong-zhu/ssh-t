# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust TUI SSH client. Core code lives in `src/`:

- `src/main.rs` boots the Tokio runtime, terminal setup, and event loop.
- `src/app/` holds application state and input handling.
- `src/ssh/`, `src/sftp/`, and `src/terminal/` contain transport, file transfer, and terminal logic.
- `src/tui/` renders the Ratatui interface.
- `src/config/` and `src/cred/` manage host configuration and credential storage.

Reference docs live in `README.md` and `DEVELOPMENT.md`. Build artifacts go to `target/` and should not be committed.

## Build, Test, and Development Commands

- `cargo run` — start the TUI locally in debug mode.
- `cargo build` — compile a debug build.
- `cargo build --release` — produce an optimized binary in `target/release/ssh-t`.
- `cargo test` — run unit and integration tests.
- `cargo clippy` — check for common Rust issues.
- `cargo fmt --check` — verify formatting; use `cargo fmt` before committing if needed.

## Coding Style & Naming Conventions

Follow standard Rust style with `rustfmt` defaults: 4-space indentation, trailing commas where rustfmt adds them, and grouped `use` imports when it improves readability. Use:

- `snake_case` for functions, modules, and variables
- `PascalCase` for structs and enums
- short, focused modules under `src/<area>/mod.rs`

Prefer small, targeted changes that keep async flow, event handling, and TUI rendering separated by module boundary.

## Testing Guidelines

There is no dedicated `tests/` suite yet, so add focused unit tests near the code you change using `#[cfg(test)]` when practical. If a feature needs broader coverage, add an integration test under `tests/`. Name tests by behavior, for example `loads_default_config` or `disconnects_on_ctrl_q`.

Run `cargo test`, then `cargo clippy`, for changes in connection, SFTP, config, or terminal handling.

## Commit & Pull Request Guidelines

Current history uses Conventional Commit style (`chore: initial commit`), so keep using prefixes like `feat:`, `fix:`, `refactor:`, and `docs:`. Write imperative, scoped summaries.

PRs should include a short description, testing notes, and any manual verification steps. For TUI changes, include screenshots or terminal recordings when the behavior or layout changes.

## Security & Configuration Tips

Never commit real host configs, passwords, or private keys. User config belongs in `~/.config/ssh-t/config.toml`; passwords should stay in the OS keyring via `keyring`. Be careful when changing SSH verification behavior—`DEVELOPMENT.md` notes that `known_hosts` validation is still a gap.
