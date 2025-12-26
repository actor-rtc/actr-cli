# actr-cli

[中文](README.zh-CN.md) | English

actr-cli is the command line tool for Actor-RTC framework projects. It bootstraps projects,
manages service dependencies, discovers services on the network, and generates code from
Protocol Buffers definitions.

## Status and limitations

- The CLI entrypoint exposes: `init`, `install`, `discovery`, `gen`, and `check`.
- `init` (Rust and Swift) and `gen` are functional today.
- `install` and `discovery` depend on service components that are not registered in
  `ContainerBuilder::build`, so they will fail with "not registered" errors until the
  container wiring is implemented.
- `check` in `src/main.rs` is a placeholder implementation.
- Python and Kotlin init/codegen are not implemented yet.

## Requirements

- Rust 1.88+ and Cargo
- `protoc` in PATH
- `rustfmt` in PATH (skip with `actr gen --no-format`)

Rust codegen:

- `protoc-gen-prost` in PATH (used by `--prost_out`)
- `protoc-gen-actrframework` in PATH
  - If missing, `actr gen` attempts to build and install it from an Actr workspace that
    contains `crates/framework-protoc-codegen`.

Swift init/codegen:

- `protoc-gen-swift`
- `protoc-gen-actrframework-swift`
- `xcodegen`
- `project.yml` present in the project root for `xcodegen generate`

## Install (from source)

```bash
cargo build --release
```

Or install the binary into your Cargo bin directory:

```bash
cargo install --path .
```

## Quick start

Create a Rust project:

```bash
actr init my-service --signaling ws://127.0.0.1:8080
cd my-service
actr gen
```

Create a Swift project (template: `echo` only):

```bash
actr init my-app --signaling ws://127.0.0.1:8080 --language swift --template echo
```

## Commands

### `actr init`

Initialize a new project. If required fields are missing, the command will prompt
interactively.

Flags:

- `--template <name>`: project template (Swift supports `echo` only)
- `--project-name <name>`: project name when initializing in the current directory
- `--signaling <url>`: signaling server URL (required)
- `-l, --language <rust|python|swift|kotlin>`: target language (default: `rust`)

Examples:

```bash
# New directory
actr init my-service --signaling ws://127.0.0.1:8080

# Current directory
actr init . --project-name my-service --signaling ws://127.0.0.1:8080

# Swift
actr init my-app --signaling ws://127.0.0.1:8080 -l swift --template echo
```

### `actr install`

Install service dependencies from `Actr.toml` or add new dependencies by package spec.

Flags:

- `--force`: reserved (not wired yet)
- `--force-update`: reserved (not wired yet)
- `--skip-verification`: reserved (not wired yet)

Examples:

```bash
# Install dependencies listed in Actr.toml
actr install

# Add a dependency
actr install actr://user-service@1.0.0/
```

### `actr discovery`

Discover services on the network and optionally add them to `Actr.toml`.
This command is interactive and will prompt for selection and actions.

Flags:

- `--filter <pattern>`: service name filter (e.g. `user-*`)
- `--verbose`: reserved (not wired yet)
- `--auto-install`: install the selected service without prompting

Example:

```bash
actr discovery --filter user-*
```

### `actr gen`

Generate code from proto files.

Flags:

- `-i, --input <path>`: input proto file or directory (default: `proto`)
- `-o, --output <path>`: output directory (default: `src/generated`)
- `--clean`: remove the output directory before generating
- `--no-scaffold`: skip user code scaffold generation
- `--overwrite-user-code`: overwrite existing user code files
- `--no-format`: skip `rustfmt`
- `--debug`: keep intermediate generated files
- `-l, --language <rust|python|swift|kotlin>`: target language (default: `rust`)

Examples:

```bash
# Rust (defaults)
actr gen

# Rust with explicit paths
actr gen -i proto -o src/generated

# Swift
actr gen -l swift -i protos/echo.proto -o MyApp/Generated
```

Notes:

- Rust codegen runs `rustfmt` and `cargo check` automatically unless `--no-format` is set.
- Generated Rust files are set to read-only after generation.
- Swift codegen runs `xcodegen generate` and requires `project.yml`.
- Python/Kotlin generators are placeholders and do not emit code yet.

### `actr check`

The CLI currently exposes a placeholder `check` command that prints the provided
flags. The full `CheckCommand` implementation exists in `src/commands/check.rs`
but is not wired into the CLI entrypoint yet.

## Configuration (`Actr.toml`)

`Actr.toml` is used by multiple commands (notably `install` and `gen`) and should
define the Actor type under `[package.actr_type]`.

Minimal example:

```toml
edition = 1
exports = []

[package]
name = "example-service"
description = "An Actor-RTC service"

[package.actr_type]
manufacturer = "acme"
name = "example-service"

[dependencies]
# "acme:other-service" = {}

[system.signaling]
url = "ws://127.0.0.1:8080"
```

## License

Apache-2.0. See `LICENSE`.
