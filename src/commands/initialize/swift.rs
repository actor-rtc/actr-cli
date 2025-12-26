use super::{InitContext, ProjectInitializer};
use crate::commands::SupportedLanguage;
use crate::error::{ActrCliError, Result};
use crate::template::{ProjectTemplate, TemplateContext};
use std::path::Path;
use std::process::Command;
use tracing::info;

pub struct SwiftInitializer;

impl ProjectInitializer for SwiftInitializer {
    fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        let template = ProjectTemplate::new(context.template, SupportedLanguage::Swift);

        let template_context = TemplateContext::new(&context.project_name, &context.signaling_url);

        template.generate(&context.project_dir, &template_context)?;

        ensure_xcodegen_available()?;
        run_xcodegen_generate(&context.project_dir)?;

        Ok(())
    }

    fn print_next_steps(&self, context: &InitContext) {
        let template_context = TemplateContext::new(&context.project_name, &context.signaling_url);
        info!("");
        info!("Next steps:");
        if !context.is_current_dir {
            info!("  cd {}", context.project_dir.display());
        }
        info!(
            "  actr gen -l swift -i protos/echo.proto -o {}/Generated",
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
