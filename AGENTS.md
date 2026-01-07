# Repository Guidelines

## Project Structure & Module Organization
- `src/` houses Rust CLI code. `src/main.rs` is the binary entrypoint and `src/lib.rs` contains shared modules. `src/commands/` implements subcommands, `src/core/` holds core logic, and `src/templates/` contains the project template system.
- `fixtures/` stores template assets (scaffolding) and test inputs.
- `tests/` contains integration tests (currently `tests/integration_test.rs`).
- `scripts/` has release helpers; `target/` is Cargo build output.

## Build, Test, and Development Commands
- `cargo build` compiles a debug build of the CLI.
- `cargo build --release` produces an optimized binary for releases.
- `cargo install --path .` installs the local `actr` binary into your Cargo bin.
- `cargo fmt` formats Rust code with rustfmt.
- After each change, run `cargo fmt` and `cargo check`.
- `cargo test` runs the test suite (optional for changes unless explicitly requested).
- Example: `cargo build --release && cargo fmt`.

## Tooling & Requirements
- Requires Rust 1.88+, Cargo, and `rustfmt`.
- `protoc` must be in PATH for codegen; Swift workflows also need `protoc-gen-swift`, `protoc-gen-actrframework-swift`, and `xcodegen`.
- Configuration for generated projects uses `Actr.toml` (see README for a minimal example).

## Coding Style & Naming Conventions
- Follow rustfmt defaults; do not hand-align.
- Naming: `snake_case` for modules/functions/vars, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep CLI output strings in English and consistent with existing phrasing.
- Prefer small modules and clear error handling using `anyhow`/`thiserror`.
- When updating README content, keep `README.md` and `README.zh-CN.md` fully aligned in structure and information.

## Testing Guidelines
- Add tests in `tests/` for behavior that touches CLI flows or codegen. Use descriptive snake_case test names.
- Run `cargo test` when you change core logic or templates; otherwise optional.

## Commit & Pull Request Guidelines
- Commit history favors Conventional Commits: `feat(scope): ...`, `fix: ...`, `chore: ...`, `refactor: ...`. Keep subjects short and imperative (no trailing period).
- PRs should include a summary, rationale, and the commands run (e.g., `cargo build`, `cargo fmt`).
- Call out user-facing CLI changes or template updates, and link related issues when available.
