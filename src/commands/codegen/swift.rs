use crate::commands::codegen::traits::{GenContext, LanguageGenerator};
use crate::error::{ActrCliError, Result};
use crate::utils::{command_exists, to_pascal_case};
use async_trait::async_trait;
use handlebars::Handlebars;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::{debug, info};
use walkdir::WalkDir;

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

        // 1. Separate local and remote files, and build relative paths
        let mut remote_paths = Vec::new();
        let mut local_paths = Vec::new();

        for proto_file in &context.proto_files {
            let is_remote = proto_file.to_string_lossy().contains("/remote/");
            let relative_path = proto_file.strip_prefix(proto_root).unwrap_or(proto_file);
            let path_str = relative_path.to_string_lossy().to_string();

            if is_remote {
                remote_paths.push(path_str);
            } else {
                local_paths.push(path_str);
            }
        }

        // 2. Build the unified options string
        let mut options = format!(
            "Visibility=Public,manufacturer={}",
            context.config.package.actr_type.manufacturer
        );

        if !remote_paths.is_empty() {
            options.push_str(&format!(",RemoteFiles={}", remote_paths.join(":")));
        }

        if !local_paths.is_empty() {
            options.push_str(&format!(",LocalFiles={}", local_paths.join(":")));
            // Keep LocalFile for backward compatibility with older plugin versions
            options.push_str(&format!(",LocalFile={}", local_paths[0]));
        }

        // Step 1: Generate basic Swift protobuf types for all files at once
        let mut cmd = StdCommand::new("protoc");
        cmd.arg(format!("--proto_path={}", proto_root.display()))
            .arg(format!("--swift_out={}", context.output.display()))
            .arg("--swift_opt=Visibility=Public");

        for proto_file in &context.proto_files {
            cmd.arg(proto_file);
        }

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

        // Step 2: Generate Actor framework code using protoc-gen-actrframework-swift for all files at once
        let mut cmd = StdCommand::new("protoc");
        cmd.arg(format!("--proto_path={}", proto_root.display()))
            .arg(format!("--actrframework-swift_opt={}", options))
            .arg(format!(
                "--actrframework-swift_out={}",
                context.output.display()
            ));

        for proto_file in &context.proto_files {
            cmd.arg(proto_file);
        }

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

        // Flatten directory structure: move all swift files from subdirectories to output root
        self.flatten_output_directory(&context.output)?;

        // Collect generated files (recursively)
        for entry in walkdir::WalkDir::new(&context.output)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "swift") {
                generated_files.push(path.to_path_buf());
            }
        }

        info!("✅ Infrastructure code generation completed");
        Ok(generated_files)
    }

    async fn generate_scaffold(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("📝 Generating Swift user code scaffold...");
        let mut scaffold_files = Vec::new();

        // 1. Parse local services to get methods for handler implementation
        let services = self.parse_local_services(context);

        // 2. Determine service name for scaffolding
        let service_name = if let Some(service) = services.first() {
            service.name.clone()
        } else if let Some(dep) = context.config.dependencies.first() {
            let type_name = dep
                .actr_type
                .as_ref()
                .map(|t| t.name.clone())
                .unwrap_or_else(|| dep.name.clone());

            debug!("Using service name from dependencies: {}", type_name);
            type_name
        } else {
            // Fallback to the first proto file name
            let guessed_name = context
                .proto_files
                .first()
                .and_then(|f| f.file_stem())
                .and_then(|s| s.to_str())
                .map(to_pascal_case)
                .map(|s| format!("{}Service", s))
                .unwrap_or_else(|| "UnknownService".to_string());

            debug!("Fallback to guessed service name: {}", guessed_name);
            guessed_name
        };

        let workload_name = if let Some(service) = services.first() {
            format!("{}Workload", service.name)
        } else {
            format!("{}Workload", to_pascal_case(&context.config.package.name))
        };

        let user_file_path = context
            .output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("ActrService.swift");

        // Check if file exists and should be overwritten
        if user_file_path.exists() {
            let is_scaffold = self.should_overwrite_scaffold(&user_file_path)?;

            // Always overwrite scaffold files (generated by init)
            if is_scaffold {
                info!("🔄 Overwriting scaffold file: {:?}", user_file_path);
            } else if !context.overwrite_user_code {
                // Skip non-scaffold files unless overwrite is forced
                info!("⏭️  Skipping existing user code file: {:?}", user_file_path);
                return Ok(scaffold_files);
            } else {
                info!(
                    "🔄 Overwriting existing file (--overwrite-user-code): {:?}",
                    user_file_path
                );
            }
        }

        let scaffold_content = self.generate_scaffold_content(
            &context.config.package.actr_type.manufacturer,
            &service_name,
            &workload_name,
            &services,
        )?;

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

    /// Flattens the output directory structure by moving all swift files from
    /// subdirectories to the root of the output directory.
    fn flatten_output_directory(&self, output_dir: &Path) -> Result<()> {
        let mut files_to_move = Vec::new();

        // Collect all swift files from subdirectories
        for entry in WalkDir::new(output_dir)
            .min_depth(2) // Skip the root directory itself
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "swift") {
                files_to_move.push(path.to_path_buf());
            }
        }

        // Move each file to the output root, overwriting existing files
        for src_path in files_to_move {
            let file_name = src_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    ActrCliError::config_error("Failed to get filename from path".to_string())
                })?;

            let mut dst_path = output_dir.to_path_buf();
            dst_path.push(file_name);

            // Overwrite existing files if they are not the same as src_path
            if dst_path.exists() && dst_path != src_path {
                debug!("Overwriting existing file: {:?}", dst_path);
                std::fs::remove_file(&dst_path).map_err(|e| {
                    ActrCliError::config_error(format!(
                        "Failed to remove existing file {:?}: {}",
                        dst_path, e
                    ))
                })?;
            }

            std::fs::rename(&src_path, &dst_path).map_err(|e| {
                ActrCliError::config_error(format!(
                    "Failed to move {} to {}: {}",
                    src_path.display(),
                    dst_path.display(),
                    e
                ))
            })?;
        }

        // Remove empty subdirectories
        self.remove_empty_subdirectories(output_dir)?;

        Ok(())
    }

    /// Recursively removes empty subdirectories from the output directory.
    fn remove_empty_subdirectories(&self, dir: &Path) -> Result<()> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    self.remove_empty_subdirectories(&path)?;
                    // Only remove if still empty after processing children
                    if path.read_dir()?.next().is_none() {
                        std::fs::remove_dir(&path)?;
                    }
                }
            }
        }
        Ok(())
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
}

#[derive(Serialize, Clone)]
struct ProtoService {
    name: String,
    package: String,
    swift_package_prefix: String,
    methods: Vec<ProtoMethod>,
}

#[derive(Serialize, Clone)]
struct ProtoMethod {
    name: String,
    swift_name: String,
    input_type: String,
    output_type: String,
}

impl SwiftGenerator {
    fn parse_local_services(&self, context: &GenContext) -> Vec<ProtoService> {
        let mut services = Vec::new();

        for proto_file_path in &context.proto_files {
            // Only look at local files
            if proto_file_path.to_string_lossy().contains("/remote/") {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(proto_file_path) {
                let mut current_package = String::new();
                let mut current_service: Option<ProtoService> = None;

                for line in content.lines() {
                    let line = line.trim();

                    // Parse package
                    if line.starts_with("package ") {
                        current_package = line
                            .trim_start_matches("package ")
                            .trim_end_matches(';')
                            .trim()
                            .to_string();
                        continue;
                    }

                    // Parse service start
                    if line.starts_with("service ") {
                        let service_name = line
                            .trim_start_matches("service ")
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_end_matches('{')
                            .trim()
                            .to_string();

                        if !service_name.is_empty() {
                            let swift_package_prefix = if current_package.is_empty() {
                                String::new()
                            } else {
                                // Convert echo_app to EchoApp_
                                let parts: Vec<String> = current_package
                                    .split('_')
                                    .map(|s| {
                                        let mut c = s.chars();
                                        match c.next() {
                                            None => String::new(),
                                            Some(f) => {
                                                f.to_uppercase().collect::<String>() + c.as_str()
                                            }
                                        }
                                    })
                                    .collect();
                                parts.join("") + "_"
                            };

                            current_service = Some(ProtoService {
                                name: service_name,
                                package: current_package.clone(),
                                swift_package_prefix,
                                methods: Vec::new(),
                            });
                        }
                        continue;
                    }

                    // Parse rpc method
                    if let Some(ref mut service) = current_service {
                        if line.starts_with("rpc ") {
                            // rpc RelayMessage(RelayMessageRequest) returns (RelayMessageResponse);
                            let line = line.trim_start_matches("rpc ").trim();
                            if let Some(name_end) = line.find('(') {
                                let method_name = line[..name_end].trim().to_string();

                                // swift_name: RelayMessage -> relayMessage
                                let mut chars = method_name.chars();
                                let swift_name = match chars.next() {
                                    None => String::new(),
                                    Some(f) => {
                                        f.to_lowercase().collect::<String>() + chars.as_str()
                                    }
                                };

                                let rest = &line[name_end + 1..];
                                if let Some(input_end) = rest.find(')') {
                                    let input_type = rest[..input_end].trim().to_string();

                                    if let Some(returns_pos) = rest.find("returns") {
                                        let rest = &rest[returns_pos + 7..];
                                        if let Some(output_start) = rest.find('(') {
                                            if let Some(output_end) = rest.find(')') {
                                                let output_type = rest
                                                    [output_start + 1..output_end]
                                                    .trim()
                                                    .to_string();

                                                service.methods.push(ProtoMethod {
                                                    name: method_name,
                                                    swift_name,
                                                    input_type: format!(
                                                        "{}{}",
                                                        service.swift_package_prefix, input_type
                                                    ),
                                                    output_type: format!(
                                                        "{}{}",
                                                        service.swift_package_prefix, output_type
                                                    ),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if line.contains('}') {
                            if let Some(s) = current_service.take() {
                                services.push(s);
                            }
                        }
                    }
                }
            }
        }
        services
    }

    fn generate_scaffold_content(
        &self,
        manufacturer: &str,
        service_name: &str,
        workload_name: &str,
        services: &[ProtoService],
    ) -> Result<String> {
        #[derive(Serialize)]
        struct SwiftScaffoldContext {
            #[serde(rename = "MANUFACTURER")]
            manufacturer: String,
            #[serde(rename = "SERVICE_NAME")]
            service_name: String,
            #[serde(rename = "WORKLOAD_NAME")]
            workload_name: String,
            #[serde(rename = "SERVICES")]
            services: Vec<ProtoService>,
            #[serde(rename = "HAS_SERVICES")]
            has_services: bool,
        }

        let context = SwiftScaffoldContext {
            manufacturer: manufacturer.to_string(),
            service_name: service_name.to_string(),
            workload_name: workload_name.to_string(),
            services: services.to_vec(),
            has_services: !services.is_empty(),
        };

        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(handlebars::no_escape);
        Ok(handlebars.render_template(ACTR_SERVICE_TEMPLATE, &context)?)
    }
}
