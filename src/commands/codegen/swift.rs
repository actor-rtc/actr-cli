use crate::commands::codegen::traits::{GenContext, LanguageGenerator};
use crate::error::{ActrCliError, Result};
use crate::utils::{command_exists, to_pascal_case};
use async_trait::async_trait;
use handlebars::Handlebars;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::{debug, info};

const ACTR_SERVICE_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/swift/ActrService.swift.hbs"
));

// Required tools for Swift codegen
const PROTOC: &str = "protoc";
const PROTOC_GEN_SWIFT: &str = "protoc-gen-swift";
const PROTOC_GEN_ACTR_FRAMEWORK_SWIFT: &str = "protoc-gen-actrframework-swift";
const REQUIRED_TOOLS: &[(&str, &str)] = &[
    (PROTOC, "Protocol Buffers compiler"),
    (PROTOC_GEN_SWIFT, "Protocol Buffers Swift codegen plugin"),
    (
        PROTOC_GEN_ACTR_FRAMEWORK_SWIFT,
        "ActrFramework Swift codegen plugin",
    ),
];

pub struct SwiftGenerator;

#[async_trait]
impl LanguageGenerator for SwiftGenerator {
    async fn generate_infrastructure(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("🔧 Generating Swift infrastructure code...");
        let mut generated_files = Vec::new();

        self.ensure_required_tools()?;

        // Ensure output directory exists
        std::fs::create_dir_all(&context.output).map_err(|e| {
            ActrCliError::config_error(format!("Failed to create output directory: {e}"))
        })?;

        let proto_root = if context.input_path.is_file() {
            context
                .input_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
        } else {
            context.input_path.as_path()
        };

        for proto_file in &context.proto_files {
            info!("Processing proto file: {:?}", proto_file);

            // Step 1: Generate basic Swift protobuf types
            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", proto_root.display()))
                .arg(format!("--swift_out={}", context.output.display()))
                .arg("--swift_opt=Visibility=Public")
                .arg(proto_file);

            debug!("Executing protoc (swift): {:?}", cmd);
            let output = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!("Failed to execute protoc (swift): {e}"))
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (swift) execution failed: {stderr}"
                )));
            }

            // Step 2: Generate Actor framework code using protoc-gen-actrframework-swift
            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", proto_root.display()))
                // .arg(format!(
                //     "--plugin=protoc-gen-actrframework-swift={}",
                //     plugin_path.display()
                // ))
                .arg(format!(
                    "--actrframework-swift_opt=manufacturer={}",
                    context.manufacturer
                ))
                .arg(format!(
                    "--actrframework-swift_out={}",
                    context.output.display()
                ))
                .arg(proto_file);

            debug!("Executing protoc (actrframework-swift): {:?}", cmd);
            let output = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!(
                    "Failed to execute protoc (actrframework-swift): {e}"
                ))
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (actrframework-swift) execution failed: {stderr}"
                )));
            }
        }

        // Collect generated files
        if let Ok(entries) = std::fs::read_dir(&context.output) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "swift") {
                    generated_files.push(path);
                }
            }
        }

        info!("✅ Infrastructure code generation completed");
        Ok(generated_files)
    }

    async fn generate_scaffold(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("📝 Generating Swift user code scaffold...");
        let mut scaffold_files = Vec::new();

        let service_names = context
            .proto_files
            .iter()
            .map(|proto_file| {
                let service_name = proto_file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| ActrCliError::config_error("Invalid proto file name"))?;
                Ok(to_pascal_case(service_name))
            })
            .collect::<Result<Vec<_>>>()?;

        let user_file_path = context
            .output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("ActrService.swift");

        // If file exists and overwrite is not forced, skip
        if user_file_path.exists()
            && !context.overwrite_user_code
            && !self.should_overwrite_scaffold(&user_file_path)?
        {
            info!("⏭️  Skipping existing user code file: {:?}", user_file_path);
            return Ok(scaffold_files);
        }

        let scaffold_content =
            self.generate_scaffold_content(&service_names, &context.manufacturer)?;

        std::fs::write(&user_file_path, scaffold_content).map_err(|e| {
            ActrCliError::config_error(format!("Failed to write user code scaffold: {e}"))
        })?;

        info!("📄 Generated user code scaffold: {:?}", user_file_path);
        scaffold_files.push(user_file_path);

        info!("✅ User code scaffold generation completed");
        Ok(scaffold_files)
    }

    async fn format_code(&self, _context: &GenContext, _files: &[PathBuf]) -> Result<()> {
        // Swift code formatting is usually done via Xcode or swift-format.
        // For now, we'll skip it as we don't want to enforce a specific tool.
        Ok(())
    }

    async fn validate_code(&self, context: &GenContext) -> Result<()> {
        info!("🔍 Running xcodegen generate...");
        self.ensure_xcodegen_available()?;
        let project_root = self.find_xcodegen_root(context)?;
        let output = StdCommand::new("xcodegen")
            .arg("generate")
            .current_dir(&project_root)
            .output()
            .map_err(|e| ActrCliError::command_error(format!("Failed to run xcodegen: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ActrCliError::command_error(format!(
                "xcodegen generate failed: {stderr}"
            )));
        }

        info!("✅ xcodegen generate completed");
        Ok(())
    }

    fn print_next_steps(&self, context: &GenContext) {
        let project_name = context
            .output
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("YourProject");

        println!("\n🎉 Swift code generation completed!");
        println!("\n📋 Next steps:");
        println!("1. 📖 View generated code: {:?}", context.output);
        if !context.no_scaffold {
            println!("2. ✏️  Implement business logic in ActrService.swift");
            println!("3. 🏗️  xcodegen generate has been run to update your Xcode project");
            println!("4. 🚀 Open {}.xcodeproj and build", project_name);
        } else {
            println!("2. 🏗️  xcodegen generate has been run to update your Xcode project");
            println!("3. 🚀 Open {}.xcodeproj and build", project_name);
        }
        println!("\n💡 Tip: Check the detailed user guide in the generated user code files");
    }
}

impl SwiftGenerator {
    fn ensure_required_tools(&self) -> Result<()> {
        let mut missing_tools = Vec::new();
        for (tool, description) in REQUIRED_TOOLS {
            if !command_exists(tool) {
                missing_tools.push((tool, description));
            }
        }

        if !missing_tools.is_empty() {
            let mut error_msg = "Missing required tools:\n".to_string();
            for (tool, description) in missing_tools {
                error_msg.push_str(&format!("  - {tool} ({description})\n"));
            }
            error_msg.push_str("\nPlease install the missing tools and try again.");
            return Err(ActrCliError::command_error(error_msg));
        }

        Ok(())
    }

    fn should_overwrite_scaffold(&self, path: &Path) -> Result<bool> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => return Ok(false),
        };

        let markers = [
            "ActrService is not implemented",
            "ActrService is not generated",
        ];

        Ok(markers.iter().any(|marker| content.contains(marker)))
    }

    fn ensure_xcodegen_available(&self) -> Result<()> {
        if command_exists("xcodegen") {
            return Ok(());
        }

        Err(ActrCliError::command_error(
            "xcodegen not found. Install via `brew install xcodegen`.".to_string(),
        ))
    }

    fn find_xcodegen_root(&self, context: &GenContext) -> Result<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd);
        }

        candidates.push(context.output.clone());

        if let Some(parent) = context.output.parent() {
            candidates.push(parent.to_path_buf());
            if let Some(grand_parent) = parent.parent() {
                candidates.push(grand_parent.to_path_buf());
            }
        }

        if context.input_path.is_dir() {
            candidates.push(context.input_path.clone());
        } else if let Some(parent) = context.input_path.parent() {
            candidates.push(parent.to_path_buf());
        }

        for candidate in candidates {
            for ancestor in candidate.ancestors() {
                if ancestor.join("project.yml").exists() {
                    return Ok(ancestor.to_path_buf());
                }
            }
        }

        Err(ActrCliError::config_error(
            "project.yml not found; cannot run xcodegen generate",
        ))
    }

    fn generate_scaffold_content(
        &self,
        service_names: &[String],
        manufacturer: &str,
    ) -> Result<String> {
        let services_hint = if service_names.is_empty() {
            "your services".to_string()
        } else {
            service_names.join(", ")
        };

        let has_echo_service = service_names.iter().any(|name| name == "Echo");
        #[derive(Serialize)]
        struct SwiftScaffoldContext {
            manufacturer: String,
            has_echo_service: bool,
            services_hint: String,
        }

        let context = SwiftScaffoldContext {
            manufacturer: manufacturer.to_string(),
            has_echo_service,
            services_hint,
        };

        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(handlebars::no_escape);
        Ok(handlebars.render_template(ACTR_SERVICE_TEMPLATE, &context)?)
    }
}
