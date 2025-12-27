use super::{InitContext, ProjectInitializer};

use crate::commands::SupportedLanguage;
use crate::error::{ActrCliError, Result};
use crate::template::{ProjectTemplate, TemplateContext};
use std::path::Path;
use std::process::Command;
use tracing::info;

pub struct PythonInitializer;

impl ProjectInitializer for PythonInitializer {
    fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        let template = ProjectTemplate::new(context.template, SupportedLanguage::Python);
        let template_context = TemplateContext::new(&context.project_name, &context.signaling_url);
        template.generate(&context.project_dir, &template_context)?;

        run_actr_gen(&context.project_dir)?;

        Ok(())
    }

    fn print_next_steps(&self, context: &InitContext) {
        info!("");
        info!("Next steps:");
        if !context.is_current_dir {
            info!("  cd {}", context.project_dir.display());
        }
        info!("  cd server");
        info!("  python server.py --actr-toml Actr.toml");
        info!("  cd ../client");
        info!("  python client.py --actr-toml Actr.toml");
    }
}

fn run_actr_gen(project_dir: &Path) -> Result<()> {
    let output = Command::new("actr")
        .arg("gen")
        .arg("--language")
        .arg("python")
        .arg("--input=proto")
        .arg("--output=generated")
        .arg("--no-scaffold")
        .current_dir(project_dir)
        .output()
        .map_err(|e| ActrCliError::Command(format!("Failed to run actr gen: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ActrCliError::Command(format!("actr gen failed: {stderr}")));
    }

    Ok(())
}
