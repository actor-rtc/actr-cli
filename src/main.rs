//! ACTR-CLI - Actor-RTC 命令行工具
//!
//! 基于复用架构实现的统一CLI工具，通过8个核心组件和3个操作管道
//! 提供一致的用户体验和高代码复用率。

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

// 导入核心复用组件
use actr_cli::core::{
    ActrCliError, Command, CommandContext, ContainerBuilder, ErrorReporter, ServiceContainer,
};

// 导入命令实现
use actr_cli::commands::{
    Command as LegacyCommand, DiscoveryCommand, GenCommand, InitCommand, InstallCommand,
};

/// ACTR-CLI - Actor-RTC Command Line Tool
#[derive(Parser)]
#[command(name = "actr")]
#[command(about = "Actor-RTC Command Line Tool", long_about = None, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Actor project
    Init(InitCommand),

    /// Install service dependencies
    Install(InstallCommand),

    /// Discover network services
    Discovery(DiscoveryCommand),

    /// Generate code from proto files
    Gen(GenCommand),

    /// Validate project dependencies
    Check {
        /// Show detailed information
        #[arg(long)]
        verbose: bool,

        /// Set timeout in seconds
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    let layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_line_number(true)
        .with_file(true);
    let _ = tracing_subscriber::registry().with(layer).try_init();

    // 使用 clap 解析命令行参数
    let cli = Cli::parse();

    // 构建服务容器并注册组件
    let container = build_container().await?;

    // 创建命令执行上下文
    let context = CommandContext {
        container: Arc::new(std::sync::Mutex::new(container)),
        args: actr_cli::core::CommandArgs {
            command: String::new(),
            subcommand: None,
            flags: std::collections::HashMap::new(),
            positional: Vec::new(),
        },
        working_dir: std::env::current_dir()?,
    };

    // 根据命令分发执行
    match execute_command(&cli.command, &context).await {
        Ok(result) => match result {
            actr_cli::core::CommandResult::Success(msg) => {
                if msg != "Help displayed" {
                    println!("{msg}");
                }
            }
            actr_cli::core::CommandResult::Install(install_result) => {
                println!("Installation complete: {}", install_result.summary());
            }
            actr_cli::core::CommandResult::Validation(validation_report) => {
                let formatted = ErrorReporter::format_validation_report(&validation_report);
                println!("{formatted}");
            }
            actr_cli::core::CommandResult::Generation(gen_result) => {
                println!("Generated {} files", gen_result.generated_files.len());
            }
            actr_cli::core::CommandResult::Error(error) => {
                eprintln!("❌ {error}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            // 统一的错误处理
            if let Some(cli_error) = e.downcast_ref::<ActrCliError>() {
                eprintln!("{}", ErrorReporter::format_error(cli_error));
            } else {
                eprintln!("Error: {e}");
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

/// 构建服务容器
async fn build_container() -> Result<ServiceContainer> {
    let container = ContainerBuilder::new().config_path("Actr.toml").build()?;

    // TODO: 在实际实现中，这里应该注册具体的组件实现
    // 例如:
    // container
    //     .register_config_manager(Arc::new(TomlConfigManager::new("Actr.toml")))
    //     .register_dependency_resolver(Arc::new(DefaultDependencyResolver::new()))
    //     .register_service_discovery(Arc::new(NetworkServiceDiscovery::new()))
    //     ...

    Ok(container)
}

/// 执行命令
async fn execute_command(
    command: &Commands,
    context: &CommandContext,
) -> Result<actr_cli::core::CommandResult> {
    match command {
        Commands::Init(cmd) => {
            // InitCommand 使用旧的 Command trait，直接执行
            match cmd.execute().await {
                Ok(_) => Ok(actr_cli::core::CommandResult::Success(
                    "Project initialized".to_string(),
                )),
                Err(e) => Err(e.into()),
            }
        }
        Commands::Install(cmd) => {
            let command = InstallCommand::from_args(cmd);

            // 验证所需组件
            {
                let container = context.container.lock().unwrap();
                container.validate(&command.required_components())?;
            }

            // 执行命令
            command.execute(context).await
        }
        Commands::Discovery(cmd) => {
            let command = DiscoveryCommand::from_args(cmd);

            // 验证所需组件
            {
                let container = context.container.lock().unwrap();
                container.validate(&command.required_components())?;
            }

            // 执行命令
            command.execute(context).await
        }
        Commands::Check { verbose, timeout } => {
            // TODO: 实现 check 命令
            if *verbose {
                println!("Check mode: verbose");
            }
            if let Some(t) = timeout {
                println!("Timeout: {} seconds", t);
            }
            Ok(actr_cli::core::CommandResult::Success(
                "Check completed".to_string(),
            ))
        }
        Commands::Gen(cmd) => match cmd.execute().await {
            Ok(_) => Ok(actr_cli::core::CommandResult::Success(
                "Generation completed".to_string(),
            )),
            Err(e) => Err(e.into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_simple() {
        // SAFETY: This is safe in a single-threaded test environment
        // We're only setting an env var that won't be used by other threads
        unsafe {
            std::env::set_var("args", "actr install user-service");
        }
        // TODO: 实现参数解析测试
    }

    #[tokio::test]
    async fn test_build_container() {
        let container = build_container().await;
        assert!(container.is_ok());
    }
}
