use crate::commands::SupportedLanguage;
use crate::commands::initialize::traits::{InitContext, ProjectInitializer};
use crate::commands::initialize::{create_local_proto, create_protoc_plugin_config};
use crate::error::Result;
use crate::template::{ProjectTemplate, TemplateContext};
use async_trait::async_trait;
use tracing::info;

pub struct RustInitializer;

#[async_trait]
impl ProjectInitializer for RustInitializer {
    async fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        info!("⚡ Generating Rust project structure...");

        // 1. Initialize with templates
        let template = ProjectTemplate::new(context.template, SupportedLanguage::Rust);
        let template_context = TemplateContext::new(
            &context.project_name,
            &context.signaling_url,
            context.template.to_service_name(),
        );

        template.generate(&context.project_dir, &template_context)?;

        // 2. Create local.proto
        create_local_proto(
            &context.project_dir,
            &context.project_name,
            "protos/local",
            context.template,
        )?;

        // 3. Create .protoc-plugin.toml
        create_protoc_plugin_config(&context.project_dir)?;

        Ok(())
    }

    fn print_next_steps(&self, context: &InitContext) {
        println!("\nNext steps:");
        if !context.is_current_dir {
            println!("  cd {}", context.project_dir.display());
        }
        println!("  actr install  # Install remote protobuf dependencies from Actr.toml");
        println!("  actr gen      # Generate Actor code");
        println!("  cargo run     # Start your work");
    }
}
