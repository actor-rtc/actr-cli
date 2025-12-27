use crate::commands::codegen::traits::{GenContext, LanguageGenerator};
use crate::error::{ActrCliError, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::{debug, info};

pub struct PythonGenerator;

#[async_trait]
impl LanguageGenerator for PythonGenerator {
    async fn generate_infrastructure(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("🐍 Generating Python code...");

        let plugin_path = ensure_python_plugin()?;

        for proto_file in &context.proto_files {
            let proto_dir = proto_file.parent().unwrap_or_else(|| Path::new("."));

            debug!("Processing proto file: {:?}", proto_file);

            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", proto_dir.display()))
                .arg(format!("--python_out={}", context.output.display()))
                .arg(proto_file);

            debug!("Running protoc (python): {:?}", cmd);
            let output = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!("Failed to run protoc (python): {e}"))
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (python) failed: {stderr}"
                )));
            }

            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", proto_dir.display()))
                .arg(format!(
                    "--plugin=protoc-gen-actrpython={}",
                    plugin_path.display()
                ))
                .arg(format!("--actrpython_out={}", context.output.display()))
                .arg(proto_file);

            debug!("Running protoc (actrpython): {:?}", cmd);
            let output = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!("Failed to run protoc (actrpython): {e}"))
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (actrpython) failed: {stderr}"
                )));
            }
        }

        info!("✅ Python code generation completed");
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
        info!("💡 Python validation is not implemented, skipping.");
        Ok(())
    }

    fn print_next_steps(&self, _context: &GenContext) {
        info!("💡 Python files are generated; add the output directory to PYTHONPATH.");
    }
}

fn ensure_python_plugin() -> Result<PathBuf> {
    if let Some(path) = find_python_plugin()? {
        info!("✅ Using installed framework_codegen_python");
        return Ok(path);
    }

    info!("📦 framework_codegen_python not found, installing...");
    install_python_plugin("framework_codegen_python", None).or_else(|_| {
        install_python_plugin(
            "framework_codegen_python",
            Some("https://test.pypi.org/simple/"),
        )
    })?;

    find_python_plugin()?.ok_or_else(|| {
        ActrCliError::command_error(
            "framework_codegen_python not found in PATH after install".to_string(),
        )
    })
}

fn find_python_plugin() -> Result<Option<PathBuf>> {
    let output = StdCommand::new("which")
        .arg("framework_codegen_python")
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(path)))
            }
        }
        _ => Ok(None),
    }
}

fn install_python_plugin(package_name: &str, index_url: Option<&str>) -> Result<()> {
    let mut cmd = StdCommand::new("python3");
    cmd.arg("-m").arg("pip").arg("install").arg("-U");
    if let Some(index_url) = index_url {
        cmd.arg("-i").arg(index_url);
    }
    cmd.arg(package_name);

    debug!("Running: {:?}", cmd);
    let output = cmd.output();

    let output = match output {
        Ok(output) => output,
        Err(_) => {
            let mut fallback = StdCommand::new("python");
            fallback.arg("-m").arg("pip").arg("install").arg("-U");
            if let Some(index_url) = index_url {
                fallback.arg("-i").arg(index_url);
            }
            fallback.arg(package_name);
            debug!("Running: {:?}", fallback);
            fallback.output().map_err(|e| {
                ActrCliError::command_error(format!("Failed to run pip install: {e}"))
            })?
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ActrCliError::command_error(format!(
            "Failed to install plugin:\n{stderr}"
        )));
    }

    Ok(())
}
