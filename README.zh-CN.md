# actr-cli

中文 | [English](README.md)

actr-cli 是 Actor-RTC 框架项目的命令行工具，用于引导项目、管理服务依赖、
发现网络服务，以及从 Protocol Buffers 定义生成代码。

## 状态与限制

- CLI 入口提供：`init`、`install`、`discovery`、`gen`、`check`。
- `init`（Rust 与 Swift）和 `gen` 目前可用。
- `install` 与 `discovery` 依赖尚未在 `ContainerBuilder::build` 中注册的服务组件，
  因此在容器接线完成前会报 “not registered” 类错误。
- `src/main.rs` 中的 `check` 是占位实现。
- Python 与 Kotlin 的 init/codegen 还未实现。

## 环境要求

- Rust 1.88+ 与 Cargo
- PATH 中可用的 `protoc`
- PATH 中可用的 `rustfmt`（可用 `actr gen --no-format` 跳过）

Rust 代码生成：

- PATH 中可用的 `protoc-gen-prost`（用于 `--prost_out`）
- PATH 中可用的 `protoc-gen-actrframework`
  - 若缺失，`actr gen` 会尝试从包含 `crates/framework-protoc-codegen` 的 Actr workspace
    构建并安装该插件。

Swift 初始化/代码生成：

- `protoc-gen-swift`
- `protoc-gen-actrframework-swift`
- `xcodegen`
- 项目根目录存在 `project.yml`（供 `xcodegen generate` 使用）

## 从源码安装

```bash
cargo build --release
```

或将二进制安装到 Cargo bin 目录：

```bash
cargo install --path .
```

## 快速开始

创建 Rust 项目：

```bash
actr init my-service --signaling ws://127.0.0.1:8080
cd my-service
actr gen
```

创建 Swift 项目（模板仅支持 `echo`）：

```bash
actr init my-app --signaling ws://127.0.0.1:8080 --language swift --template echo
```

## 命令

### `actr init`

初始化新项目。如果缺少必填项，会进入交互式提示。

参数：

- `--template <name>`：项目模板（Swift 仅支持 `echo`）
- `--project-name <name>`：在当前目录初始化时指定项目名
- `--signaling <url>`：信令服务器地址（必填）
- `-l, --language <rust|python|swift|kotlin>`：目标语言（默认：`rust`）

示例：

```bash
# 新目录
actr init my-service --signaling ws://127.0.0.1:8080

# 当前目录
actr init . --project-name my-service --signaling ws://127.0.0.1:8080

# Swift
actr init my-app --signaling ws://127.0.0.1:8080 -l swift --template echo
```

### `actr install`

从 `Actr.toml` 安装服务依赖，或按包规格新增依赖。

参数：

- `--force`：保留（尚未接线）
- `--force-update`：保留（尚未接线）
- `--skip-verification`：保留（尚未接线）

示例：

```bash
# 安装 Actr.toml 中的依赖
actr install

# 新增依赖
actr install actr://user-service@1.0.0/
```

### `actr discovery`

发现网络中的服务，并可选写入 `Actr.toml`。
该命令为交互式，会提示选择与操作。

参数：

- `--filter <pattern>`：服务名过滤（例如 `user-*`）
- `--verbose`：保留（尚未接线）
- `--auto-install`：不提示直接安装选中服务

示例：

```bash
actr discovery --filter user-*
```

### `actr gen`

从 proto 文件生成代码。

参数：

- `-i, --input <path>`：输入的 proto 文件或目录（默认：`proto`）
- `-o, --output <path>`：输出目录（默认：`src/generated`）
- `--clean`：生成前清理输出目录
- `--no-scaffold`：跳过用户代码骨架生成
- `--overwrite-user-code`：覆盖已有用户代码文件
- `--no-format`：跳过 `rustfmt`
- `--debug`：保留中间生成文件
- `-l, --language <rust|python|swift|kotlin>`：目标语言（默认：`rust`）

示例：

```bash
# Rust（默认）
actr gen

# Rust 指定路径
actr gen -i proto -o src/generated

# Swift
actr gen -l swift -i protos/echo.proto -o MyApp/Generated
```

说明：

- Rust 代码生成会自动执行 `rustfmt` 与 `cargo check`（除非设置 `--no-format`）。
- 生成的 Rust 文件在生成完成后会设置为只读。
- Swift 代码生成会运行 `xcodegen generate`，并要求存在 `project.yml`。
- Python/Kotlin 生成器目前为占位实现，不会产出代码。

### `actr check`

当前 CLI 暴露的是一个占位 `check` 命令，仅打印传入的参数。
完整实现存在于 `src/commands/check.rs`，但尚未接入 CLI 入口。

## 配置（`Actr.toml`）

`Actr.toml` 会被多个命令使用（尤其是 `install` 与 `gen`），
并应在 `[package.actr_type]` 中定义 Actor 类型。

最小示例：

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
# "acme+other-service" = {}

[system.signaling]
url = "ws://127.0.0.1:8080"
```

## 许可证

Apache-2.0。详见 `LICENSE`。
