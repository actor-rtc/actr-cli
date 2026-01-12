use super::{InitContext, ProjectInitializer};
use crate::commands::SupportedLanguage;
use crate::error::{ActrCliError, Result};
use crate::template::{ProjectTemplate, TemplateContext};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use tracing::info;

pub struct SwiftInitializer;

#[async_trait]
impl ProjectInitializer for SwiftInitializer {
    async fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        let template = ProjectTemplate::new(context.template, SupportedLanguage::Swift);
        let service_name = context.template.to_service_name();

        // Fetch versions asynchronously
        let template_context = TemplateContext::new_with_versions(
            &context.project_name,
            &context.signaling_url,
            service_name,
        )
        .await;

        template.generate(&context.project_dir, &template_context)?;

        ensure_xcodegen_available()?;
        run_xcodegen_generate(&context.project_dir)?;

        // Initialize git repository
        init_git_repo(&context.project_dir)?;

        Ok(())
    }

    fn print_next_steps(&self, context: &InitContext) {
        let template_context = TemplateContext::new(
            &context.project_name,
            &context.signaling_url,
            context.template.to_service_name(),
        );
        info!("");
        info!("Next steps:");
        if !context.is_current_dir {
            info!("  cd {}", context.project_dir.display());
        }
        info!("  actr install  # Install remote protobuf dependencies from Actr.toml");
        info!(
            "  actr gen -l swift -i protos/remote/{{service-name}}/{{proto-file}} -o {}/Generated",
            template_context.project_name_pascal
        );
        info!("  xcodegen generate");
        info!("  open {}.xcodeproj", template_context.project_name_pascal);
        info!("  # If you update project.yml, rerun: xcodegen generate");
    }
}

fn ensure_xcodegen_available() -> Result<()> {
    match Command::new("xcodegen").arg("--version").output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ActrCliError::Command(format!(
                "xcodegen is not available. Install via `brew install xcodegen`. {stderr}"
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(ActrCliError::Command(
            "xcodegen not found. Install via `brew install xcodegen`.".to_string(),
        )),
        Err(error) => Err(ActrCliError::Command(format!(
            "Failed to run xcodegen: {error}"
        ))),
    }
}

fn run_xcodegen_generate(project_dir: &Path) -> Result<()> {
    let output = Command::new("xcodegen")
        .arg("generate")
        .current_dir(project_dir)
        .output()
        .map_err(|error| ActrCliError::Command(format!("Failed to run xcodegen: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ActrCliError::Command(format!(
            "xcodegen generate failed: {stderr}"
        )));
    }

    Ok(())
}

fn init_git_repo(project_dir: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(project_dir)
        .output()
        .map_err(|error| ActrCliError::Command(format!("Failed to run git init: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ActrCliError::Command(format!("git init failed: {stderr}")));
    }

    info!("🔧 Initialized git repository");
    Ok(())
}
