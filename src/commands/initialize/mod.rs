mod kotlin;
mod python;
mod swift;
pub mod traits;

use crate::commands::SupportedLanguage;
use crate::error::{ActrCliError, Result};
use kotlin::KotlinInitializer;
use python::PythonInitializer;
use swift::SwiftInitializer;

pub use traits::{InitContext, ProjectInitializer};

pub struct InitializerFactory;

impl InitializerFactory {
    pub fn get_initializer(language: SupportedLanguage) -> Result<Box<dyn ProjectInitializer>> {
        match language {
            SupportedLanguage::Rust => Err(ActrCliError::Unsupported(
                "Rust initialization is handled by InitCommand".to_string(),
            )),
            SupportedLanguage::Python => Ok(Box::new(PythonInitializer)),
            SupportedLanguage::Swift => Ok(Box::new(SwiftInitializer)),
            SupportedLanguage::Kotlin => Ok(Box::new(KotlinInitializer)),
        }
    }
}

pub async fn execute_initialize(language: SupportedLanguage, context: &InitContext) -> Result<()> {
    let initializer = InitializerFactory::get_initializer(language)?;
    initializer.generate_project_structure(context).await?;
    initializer.print_next_steps(context);
    Ok(())
}
