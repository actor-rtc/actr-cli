mod kotlin;
mod python;
mod rust;
mod swift;
pub mod traits;

use crate::commands::SupportedLanguage;
use crate::error::{ActrCliError, Result};
use crate::template::{ProjectTemplateName, TemplateContext};
use crate::utils::read_fixture_text;
use handlebars::Handlebars;
use kotlin::KotlinInitializer;
use python::PythonInitializer;
use rust::RustInitializer;
use std::path::Path;
use swift::SwiftInitializer;

pub use traits::{InitContext, ProjectInitializer};

/// Create .protoc-plugin.toml with default minimum versions.
pub fn create_protoc_plugin_config(project_dir: &Path) -> Result<()> {
    const DEFAULT_PLUGIN_MIN_VERSION: &str = "0.1.10";

    let config_path = project_dir.join(".protoc-plugin.toml");
    if config_path.exists() {
        return Ok(());
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = format!(
        "version = 1\n\n[plugins]\nprotoc-gen-actrframework = \"{DEFAULT_PLUGIN_MIN_VERSION}\"\nprotoc-gen-actrframework-swift = \"{DEFAULT_PLUGIN_MIN_VERSION}\"\n"
    );

    std::fs::write(&config_path, content)?;
    tracing::info!("📄 Created .protoc-plugin.toml");
    Ok(())
}

/// Generate a local.proto file with the given package name
pub fn create_local_proto(
    project_dir: &Path,
    project_name: &str,
    proto_dir: &str,
    template: ProjectTemplateName,
) -> Result<()> {
    let proto_path = project_dir.join(proto_dir);
    std::fs::create_dir_all(&proto_path)?;

    // Load template file
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let template_file_name = match template {
        ProjectTemplateName::Echo => "local.echo.hbs",
        ProjectTemplateName::DataStream => "local.data-stream.hbs",
    };
    let template_path = fixtures_root.join("protos").join(template_file_name);

    let template_content = read_fixture_text(&template_path)?;

    // Create template context
    let template_context = TemplateContext::new(project_name, "", "");
    let handlebars = Handlebars::new();

    // Render template
    let local_proto_content = handlebars
        .render_template(&template_content, &template_context)
        .map_err(|e| {
            ActrCliError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to render proto template: {}", e),
            ))
        })?;

    let local_proto_path = proto_path.join("local.proto");
    std::fs::write(&local_proto_path, local_proto_content)?;

    tracing::info!("📄 Created {}", local_proto_path.display());
    Ok(())
}

pub struct InitializerFactory;

impl InitializerFactory {
    pub fn get_initializer(language: SupportedLanguage) -> Result<Box<dyn ProjectInitializer>> {
        match language {
            SupportedLanguage::Rust => Ok(Box::new(RustInitializer)),
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
