use crate::commands::codegen::traits::{GenContext, LanguageGenerator};
use crate::error::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::info;

pub struct PythonGenerator;

#[async_trait]
impl LanguageGenerator for PythonGenerator {
    async fn generate_infrastructure(&self, _context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("🔧 Python code generation is not yet implemented.");
        Ok(vec![])
    }

    async fn generate_scaffold(&self, _context: &GenContext) -> Result<Vec<PathBuf>> {
        Ok(vec![])
    }

    async fn format_code(&self, _context: &GenContext, _files: &[PathBuf]) -> Result<()> {
        Ok(())
    }

    async fn validate_code(&self, _context: &GenContext) -> Result<()> {
        info!("🔍 Validating Python code...");
        info!("💡 Python code validation is not yet implemented, skipping.");
        Ok(())
    }

    fn print_next_steps(&self, _context: &GenContext) {
        info!(
            "💡 For Python, please check the generated files and refer to the Actor-RTC Python documentation."
        );
    }
}
