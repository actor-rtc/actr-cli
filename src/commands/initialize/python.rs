use super::{InitContext, ProjectInitializer};
use crate::error::{ActrCliError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;

pub struct PythonInitializer;

impl ProjectInitializer for PythonInitializer {
    fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        let template_name = context.template.as_deref().unwrap_or("echo_demo");
        if template_name != "echo_demo" {
            return Err(ActrCliError::InvalidProject(format!(
                "Unknown template: {template_name}"
            )));
        }

        let replacements = vec![
            ("{{PROJECT_NAME}}".to_string(), context.project_name.clone()),
            (
                "{{SIGNALING_URL}}".to_string(),
                context.signaling_url.clone(),
            ),
        ];

        let fixtures_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let python_fixtures = fixtures_root.join("python");
        let python_templates =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/python/echo");

        let files = vec![
            (
                fixtures_root.join("echo.proto"),
                context.project_dir.join("proto/echo.proto"),
            ),
            (
                python_fixtures.join("Actr.server.toml"),
                context.project_dir.join("server/Actr.toml"),
            ),
            (
                python_fixtures.join("Actr.client.toml"),
                context.project_dir.join("client/Actr.toml"),
            ),
            (
                python_templates.join("server.py"),
                context.project_dir.join("server/server.py"),
            ),
            (
                python_templates.join("client.py"),
                context.project_dir.join("client/client.py"),
            ),
            (
                python_fixtures.join("README.md"),
                context.project_dir.join("README.md"),
            ),
            (
                python_fixtures.join("gitignore"),
                context.project_dir.join(".gitignore"),
            ),
        ];

        for (fixture_path, output_path) in files {
            let template = std::fs::read_to_string(&fixture_path)?;
            let rendered = apply_placeholders(&template, &replacements);
            write_file(&output_path, &rendered)?;
        }

        write_file(&context.project_dir.join("generated/__init__.py"), "")?;

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

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn apply_placeholders(template: &str, replacements: &[(String, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in replacements {
        rendered = rendered.replace(key, value);
    }
    rendered
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
