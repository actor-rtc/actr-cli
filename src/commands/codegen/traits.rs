use crate::error::Result;
use async_trait::async_trait;
use std::path::PathBuf;

/// Type of scaffold code to generate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaffoldType {
    /// Generate server-side scaffold only
    Server,
    /// Generate client-side scaffold only
    Client,
    /// Generate both server and client scaffolds
    #[default]
    Both,
}

/// Context for code generation
#[derive(Debug, Clone)]
pub struct GenContext {
    pub proto_files: Vec<PathBuf>,
    pub input_path: PathBuf,
    pub output: PathBuf,
    pub manufacturer: String,
    pub no_scaffold: bool,
    pub overwrite_user_code: bool,
    pub no_format: bool,
    pub debug: bool,
    /// Kotlin package name (for Kotlin language generation)
    pub kotlin_package: Option<String>,
    /// Scaffold type to generate (server, client, or both)
    pub scaffold_type: ScaffoldType,
}

/// Interface for language-specific code generators
#[async_trait]
pub trait LanguageGenerator: Send {
    /// Generate infrastructure code (e.g., protobuf types, actors)
    async fn generate_infrastructure(&self, context: &GenContext) -> Result<Vec<PathBuf>>;

    /// Generate user code scaffold
    async fn generate_scaffold(&self, context: &GenContext) -> Result<Vec<PathBuf>>;

    /// Format generated code using language-specific tools
    async fn format_code(&self, context: &GenContext, files: &[PathBuf]) -> Result<()>;

    /// Validate generated code (e.g., using a compiler)
    async fn validate_code(&self, context: &GenContext) -> Result<()>;

    /// Print next steps
    fn print_next_steps(&self, context: &GenContext);
}
