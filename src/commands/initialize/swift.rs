use super::{InitContext, ProjectInitializer};
use crate::error::{ActrCliError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;

pub struct SwiftInitializer;

impl ProjectInitializer for SwiftInitializer {
    fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        let template_name = context.template.as_deref().unwrap_or("echo");
        if template_name != "echo" {
            return Err(ActrCliError::InvalidProject(format!(
                "Unknown template: {template_name}"
            )));
        }

        let project_name_pascal = to_pascal_case(&context.project_name);
        let app_struct_name = format!("{project_name_pascal}App");
        let bundle_id = to_bundle_id(&project_name_pascal);

        let replacements = vec![
            ("{{PROJECT_NAME}}".to_string(), context.project_name.clone()),
            (
                "{{PROJECT_NAME_PASCAL}}".to_string(),
                project_name_pascal.clone(),
            ),
            ("{{APP_STRUCT_NAME}}".to_string(), app_struct_name),
            ("{{BUNDLE_ID}}".to_string(), bundle_id),
            (
                "{{SIGNALING_URL}}".to_string(),
                context.signaling_url.clone(),
            ),
        ];

        let fixtures_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let app_dir = context.project_dir.join(&project_name_pascal);

        let files = vec![
            (
                fixtures_root.join("swift/project.yml"),
                context.project_dir.join("project.yml"),
            ),
            (
                fixtures_root.join("swift/Actr.toml"),
                context.project_dir.join("Actr.toml"),
            ),
            (
                fixtures_root.join("swift/gitignore"),
                context.project_dir.join(".gitignore"),
            ),
            (
                fixtures_root.join("echo.proto"),
                context.project_dir.join("protos/echo.proto"),
            ),
            (
                fixtures_root.join("swift/Info.plist"),
                app_dir.join("Info.plist"),
            ),
            (
                fixtures_root.join("swift/App.swift"),
                app_dir.join(format!("{project_name_pascal}App.swift")),
            ),
            (
                fixtures_root.join("swift/ContentView.swift"),
                app_dir.join("ContentView.swift"),
            ),
            (
                fixtures_root.join("swift/ActrService.swift"),
                app_dir.join("ActrService.swift"),
            ),
            (
                fixtures_root.join("swift/actr-config.toml"),
                app_dir.join("actr-config.toml"),
            ),
            (
                fixtures_root.join("swift/Assets.xcassets/Contents.json"),
                app_dir.join("Assets.xcassets/Contents.json"),
            ),
            (
                fixtures_root.join("swift/Assets.xcassets/AccentColor.colorset/Contents.json"),
                app_dir.join("Assets.xcassets/AccentColor.colorset/Contents.json"),
            ),
            (
                fixtures_root.join("swift/Assets.xcassets/AppIcon.appiconset/Contents.json"),
                app_dir.join("Assets.xcassets/AppIcon.appiconset/Contents.json"),
            ),
        ];

        for (fixture_path, output_path) in files {
            let template = std::fs::read_to_string(&fixture_path)?;
            let rendered = apply_placeholders(&template, &replacements);
            write_file(&output_path, &rendered)?;
        }

        ensure_xcodegen_available()?;
        run_xcodegen_generate(&context.project_dir)?;

        Ok(())
    }

    fn print_next_steps(&self, context: &InitContext) {
        let project_name_pascal = to_pascal_case(&context.project_name);
        info!("");
        info!("Next steps:");
        if !context.is_current_dir {
            info!("  cd {}", context.project_dir.display());
        }
        info!(
            "  actr gen -l swift -i protos/echo.proto -o {}/Generated",
            project_name_pascal
        );
        info!("  xcodegen generate");
        info!("  open {}.xcodeproj", project_name_pascal);
        info!("  # If you update project.yml, rerun: xcodegen generate");
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

fn to_pascal_case(input: &str) -> String {
    let mut result = String::new();
    let mut start_of_word = true;

    for c in input.chars() {
        if !c.is_alphanumeric() {
            start_of_word = true;
            continue;
        }

        if c.is_uppercase() {
            result.push(c);
            start_of_word = false;
        } else if start_of_word {
            result.push(c.to_uppercase().next().unwrap_or(c));
            start_of_word = false;
        } else {
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
    }

    result
}

fn to_bundle_id(project_name_pascal: &str) -> String {
    format!("io.actr.{project_name_pascal}")
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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
            ActrCliError::Command("xcodegen not found. Install via `brew install xcodegen`.".to_string()),
        ),
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
