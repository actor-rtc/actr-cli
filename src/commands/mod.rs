//! Command implementations for actr-cli

pub mod check;
// TODO: config command needs rewrite for new Config API
// pub mod config;
pub mod codegen;
pub mod discovery;
pub mod doc;
pub mod fingerprint;
pub mod generate;
pub mod init;
pub mod initialize;
pub mod install;
pub mod run;

use crate::error::Result;
use async_trait::async_trait;
use clap::ValueEnum;

// Legacy command trait for backward compatibility
#[async_trait]
pub trait Command {
    async fn execute(&self) -> Result<()>;
}

/// Supported languages for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
pub enum SupportedLanguage {
    Rust,
    Python,
    Swift,
    Kotlin,
}

// Re-export new architecture commands
pub use discovery::DiscoveryCommand;
pub use generate::GenCommand;
pub use init::InitCommand;
pub use install::InstallCommand;
