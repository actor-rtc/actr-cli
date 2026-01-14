use crate::commands::codegen::traits::{GenContext, LanguageGenerator};
use crate::error::{ActrCliError, Result};
use crate::utils::{command_exists, to_pascal_case};
use actr_config::LockFile;
use async_trait::async_trait;
use handlebars::Handlebars;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

// Template for Python service scaffold
const ACTR_SERVICE_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/python/ActrService.py.hbs"
));

// Required tools for Python codegen
const PROTOC: &str = "protoc";
const REQUIRED_TOOLS: &[(&str, &str)] = &[(PROTOC, "Protocol Buffers compiler")];

#[derive(Serialize, Clone)]
struct ProtoService {
    name: String,
    package: String,
    methods: Vec<ProtoMethod>,
}

#[derive(Serialize, Clone)]
struct ProtoMethod {
    name: String,
    snake_name: String,
    input_type: String,
    output_type: String,
}

pub struct PythonGenerator;

#[async_trait]
impl LanguageGenerator for PythonGenerator {
    async fn generate_infrastructure(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("🔧 Generating Python infrastructure code...");
        let mut generated_files = Vec::new();

        self.ensure_required_tools()?;

        let plugin_path = ensure_python_plugin()?;

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

        // 1. Read Actr.lock.toml from current working directory
        // The lock file should always be in the project root, not in the protos directory
        let lock_file_path = PathBuf::from("Actr.lock.toml");

        // Check if lock file exists - required for code generation
        if !lock_file_path.exists() {
            return Err(ActrCliError::config_error(format!(
                "Actr.lock.toml not found at {}. Please run 'actr install' first.",
                lock_file_path.display()
            )));
        }

        // Read and parse lock file
        let lock_file = LockFile::from_file(&lock_file_path).map_err(|e| {
            ActrCliError::config_error(format!(
                "Failed to read lock file at {}: {}",
                lock_file_path.display(),
                e
            ))
        })?;

        info!("📖 Reading lock file: {}", lock_file_path.display());

        // Build remote services mapping
        let mut remote_services_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for dep in lock_file.dependencies {
            for file in dep.files {
                // Map proto file path to actr_type
                // file.path is like "data-stream-peer-concurrent-server-python/data_stream_peer.proto"
                remote_services_map.insert(file.path.clone(), dep.actr_type.clone());
            }
        }

        info!(
            "✅ Found {} remote service mappings",
            remote_services_map.len()
        );

        // 2. Separate local and remote files based on lock file
        let mut remote_paths = Vec::new();
        let mut remote_actr_types = Vec::new();
        let mut local_paths = Vec::new();

        for proto_file in &context.proto_files {
            let relative_path = proto_file.strip_prefix(proto_root).unwrap_or(proto_file);
            let path_str = relative_path.to_string_lossy().to_string();

            // Check if this file is in the lock file (under remote/)
            let is_remote = if path_str.contains("/remote/") || path_str.starts_with("remote/") {
                // Extract the path after "remote/"
                let remote_relative_path = if let Some(pos) = path_str.find("/remote/") {
                    &path_str[pos + "/remote/".len()..]
                } else if path_str.starts_with("remote/") {
                    &path_str["remote/".len()..]
                } else {
                    ""
                };

                if !remote_relative_path.is_empty() {
                    debug!("🔍 Checking remote file: {}", remote_relative_path);
                    if let Some(actr_type) = remote_services_map.get(remote_relative_path) {
                        info!(
                            "✅ Matched remote file '{}' to actr_type '{}'",
                            remote_relative_path, actr_type
                        );
                        remote_actr_types.push(actr_type.clone());
                        true
                    } else {
                        // In remote/ but not in lock file, still treat as remote
                        warn!("⚠️  Remote file not in lock file: {}", remote_relative_path);
                        warn!(
                            "    Available paths in lock: {:?}",
                            remote_services_map.keys().collect::<Vec<_>>()
                        );
                        remote_actr_types.push(String::new()); // Empty actr_type
                        true
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if is_remote {
                remote_paths.push(path_str);
            } else {
                local_paths.push(path_str);
            }
        }

        // 3. Build the unified options string
        info!("🔍 Remote files: {:?}", remote_paths);
        info!("🔍 Remote actr_types: {:?}", remote_actr_types);
        info!("🔍 Local files: {:?}", local_paths);

        let mut options = format!(
            "manufacturer={}",
            context.config.package.actr_type.manufacturer
        );

        if !remote_paths.is_empty() {
            options.push_str(&format!(",RemoteFiles={}", remote_paths.join(":")));
            options.push_str(&format!(",RemoteActrTypes={}", remote_actr_types.join(":")));
        }

        if !local_paths.is_empty() {
            options.push_str(&format!(",LocalFiles={}", local_paths.join(":")));
            // Keep LocalFile for backward compatibility with older plugin versions
            options.push_str(&format!(",LocalFile={}", local_paths[0]));
        }

        info!("📝 Options: {}", options);

        // Step 1: Generate basic Python protobuf types for all files at once
        let mut cmd = StdCommand::new("protoc");
        cmd.arg(format!("--proto_path={}", proto_root.display()))
            .arg(format!("--python_out={}", context.output.display()));

        for proto_file in &context.proto_files {
            cmd.arg(proto_file);
        }

        debug!("Executing protoc (python): {:?}", cmd);
        let output = cmd.output().map_err(|e| {
            ActrCliError::command_error(format!("Failed to execute protoc (python): {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ActrCliError::command_error(format!(
                "protoc (python) execution failed: {stderr}"
            )));
        }

        // Step 2: Generate Actor framework code using protoc-gen-actrpython for all files at once
        let mut cmd = StdCommand::new("protoc");
        cmd.arg(format!("--proto_path={}", proto_root.display()))
            .arg(format!(
                "--plugin=protoc-gen-actrpython={}",
                plugin_path.display()
            ))
            .arg(format!("--actrpython_opt={}", options))
            .arg(format!("--actrpython_out={}", context.output.display()));

        for proto_file in &context.proto_files {
            cmd.arg(proto_file);
        }

        debug!("Executing protoc (actrpython): {:?}", cmd);
        let output = cmd.output().map_err(|e| {
            ActrCliError::command_error(format!("Failed to execute protoc (actrpython): {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ActrCliError::command_error(format!(
                "protoc (actrpython) execution failed: {stderr}"
            )));
        }

        // Collect generated files (recursively)
        for entry in WalkDir::new(&context.output)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "py") {
                generated_files.push(path.to_path_buf());
            }
        }

        info!("✅ Infrastructure code generation completed");
        Ok(generated_files)
    }

    async fn generate_scaffold(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("📝 Generating Python user code scaffold...");
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

        // Determine filename based on service type
        let filename = if services.is_empty() {
            // No local services - this is a client
            "client.py".to_string()
        } else {
            // Has local services - use service name
            format!("{}.py", to_snake_case(&service_name))
        };

        let user_file_path = context
            .output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(filename);

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

        let scaffold_content =
            self.generate_scaffold_content(context, &service_name, &workload_name, &services)?;

        std::fs::write(&user_file_path, scaffold_content).map_err(|e| {
            ActrCliError::config_error(format!("Failed to write user code scaffold: {e}"))
        })?;

        info!("📄 Generated user code scaffold: {:?}", user_file_path);
        scaffold_files.push(user_file_path);

        info!("✅ User code scaffold generation completed");
        Ok(scaffold_files)
    }

    async fn format_code(&self, context: &GenContext, files: &[PathBuf]) -> Result<()> {
        // Check if black is available
        if !command_exists("black") {
            info!("💡 black not found, skipping code formatting");
            info!("   Install with: pip3 install black");
            return Ok(());
        }

        info!("🎨 Formatting Python code with black...");

        // Format all Python files in the output directory
        let output = StdCommand::new("black")
            .arg("--quiet")
            .arg(&context.output)
            .output()
            .map_err(|e| ActrCliError::command_error(format!("Failed to run black: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("⚠️  Black formatting encountered issues: {}", stderr);
            // Don't fail on formatting errors, just warn
            return Ok(());
        }

        // Also format scaffold file if it exists and is in the files list
        for file in files {
            if file.exists() && file.extension().is_some_and(|ext| ext == "py") {
                let output = StdCommand::new("black")
                    .arg("--quiet")
                    .arg(file)
                    .output()
                    .map_err(|e| {
                        ActrCliError::command_error(format!(
                            "Failed to run black on {:?}: {e}",
                            file
                        ))
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("⚠️  Black formatting failed for {:?}: {}", file, stderr);
                }
            }
        }

        info!("✅ Code formatting completed");
        Ok(())
    }

    async fn validate_code(&self, context: &GenContext) -> Result<()> {
        info!("🔍 Validating Python code...");

        // Check if python3 is available
        if !command_exists("python3") && !command_exists("python") {
            warn!("⚠️  Python not found, skipping code validation");
            return Ok(());
        }

        let python_cmd = if command_exists("python3") {
            "python3"
        } else {
            "python"
        };

        // Collect all Python files in the output directory
        let mut python_files = Vec::new();
        for entry in WalkDir::new(&context.output)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "py") {
                python_files.push(path.to_path_buf());
            }
        }

        if python_files.is_empty() {
            info!("💡 No Python files found to validate");
            return Ok(());
        }

        info!("🔍 Validating {} Python files...", python_files.len());

        // Validate each file using py_compile
        let mut failed_files = Vec::new();
        for file in &python_files {
            let output = StdCommand::new(python_cmd)
                .arg("-m")
                .arg("py_compile")
                .arg(file)
                .output()
                .map_err(|e| {
                    ActrCliError::command_error(format!("Failed to run python -m py_compile: {e}"))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("⚠️  Syntax error in {:?}: {}", file, stderr);
                failed_files.push((file.clone(), stderr.to_string()));
            }
        }

        if !failed_files.is_empty() {
            let mut error_msg = format!(
                "Python syntax validation failed for {} files:\n",
                failed_files.len()
            );
            for (file, error) in failed_files {
                error_msg.push_str(&format!("  - {:?}: {}\n", file, error));
            }
            return Err(ActrCliError::command_error(error_msg));
        }

        info!("✅ Python code validation completed successfully");
        Ok(())
    }

    fn print_next_steps(&self, context: &GenContext) {
        println!("\n🎉 Python code generation completed!");
        println!("\n📋 Next steps:");
        println!("1. 📖 View generated code: {:?}", context.output);
        println!("2. 📦 Add the output directory to PYTHONPATH:");
        println!(
            "   export PYTHONPATH=$PYTHONPATH:{}",
            context.output.display()
        );
        println!("3. 🐍 Import and use the generated modules in your Python code");
        println!("\n💡 Tip: Consider using a virtual environment for your Python project");
    }
}

impl PythonGenerator {
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

        // Check if file contains scaffold markers
        let markers = [
            "# DO NOT EDIT - Generated scaffold",
            "TODO: Implement your business logic",
            "is not implemented yet",
        ];

        Ok(markers.iter().any(|marker| content.contains(marker)))
    }

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
                            current_service = Some(ProtoService {
                                name: service_name,
                                package: current_package.clone(),
                                methods: Vec::new(),
                            });
                        }
                        continue;
                    }

                    // Parse rpc method
                    if let Some(ref mut service) = current_service {
                        if line.starts_with("rpc ") {
                            // rpc Echo(EchoRequest) returns (EchoResponse);
                            let line = line.trim_start_matches("rpc ").trim();
                            if let Some(name_end) = line.find('(') {
                                let method_name = line[..name_end].trim().to_string();

                                // Convert to snake_case
                                let snake_name = to_snake_case(&method_name);

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
                                                    snake_name,
                                                    input_type,
                                                    output_type,
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
        context: &GenContext,
        service_name: &str,
        workload_name: &str,
        services: &[ProtoService],
    ) -> Result<String> {
        #[derive(Serialize)]
        struct ScaffoldContext {
            #[serde(rename = "SERVICE_NAME")]
            service_name: String,
            #[serde(rename = "WORKLOAD_NAME")]
            workload_name: String,
            #[serde(rename = "DISPATCHER_NAME")]
            dispatcher_name: String,
            #[serde(rename = "PROTO_MODULE")]
            proto_module: String,
            #[serde(rename = "ACTOR_MODULE")]
            actor_module: String,
            #[serde(rename = "SERVICES")]
            services: Vec<ProtoService>,
            #[serde(rename = "HAS_SERVICES")]
            has_services: bool,
        }

        // Derive proto_module from the first proto file name (without .proto extension)
        let proto_module = context
            .proto_files
            .first()
            .and_then(|f| f.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "proto".to_string());

        let actor_module = if let Some(service) = services.first() {
            format!("{}_actor", to_snake_case(&service.name))
        } else {
            // For client workloads, use proto_module + "_workload"
            format!("{}_workload", proto_module)
        };

        let dispatcher_name = services
            .first()
            .map(|s| format!("{}Dispatcher", s.name))
            .unwrap_or_else(|| "Dispatcher".to_string());

        let context = ScaffoldContext {
            service_name: service_name.to_string(),
            workload_name: workload_name.to_string(),
            dispatcher_name,
            proto_module,
            actor_module,
            services: services.to_vec(),
            has_services: !services.is_empty(),
        };

        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(handlebars::no_escape);
        Ok(handlebars.render_template(ACTR_SERVICE_TEMPLATE, &context)?)
    }
}

// Helper function to convert CamelCase to snake_case
fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i != 0 {
            result.push('_');
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;
    use actr_config::{LockFile, LockedDependency, ProtoFileMeta, ServiceSpecMeta};
    use std::collections::HashMap;

    #[test]
    fn test_remote_path_extraction() {
        // Test the logic for extracting remote path after "/remote/"
        let test_cases = vec![
            (
                "protos/remote/server/service.proto",
                Some("server/service.proto"),
            ),
            // "remote/test.proto" will NOT match because split produces ["", "test.proto"]
            // which is only 2 parts, but the first part is empty, not what we want
            ("protos/remote/test.proto", Some("test.proto")),
            ("protos/local.proto", None),
            ("no_remote_here.proto", None),
        ];

        for (input, expected) in test_cases {
            let parts: Vec<&str> = input.split("/remote/").collect();
            let result = if parts.len() == 2 && !parts[0].is_empty() {
                Some(parts[1])
            } else {
                None
            };

            assert_eq!(
                result, expected,
                "Failed for input: {}, expected: {:?}, got: {:?}",
                input, expected, result
            );
        }
    }

    #[test]
    fn test_remote_services_map_construction() {
        // Create a simple mock lock file structure
        let mut remote_services_map: HashMap<String, String> = HashMap::new();

        // Simulate adding entries from lock file
        remote_services_map.insert(
            "server/service.proto".to_string(),
            "acme+TestServer".to_string(),
        );
        remote_services_map.insert(
            "api/v1/api.proto".to_string(),
            "custom+ApiService".to_string(),
        );

        // Verify the mapping
        assert_eq!(remote_services_map.len(), 2);
        assert_eq!(
            remote_services_map.get("server/service.proto"),
            Some(&"acme+TestServer".to_string())
        );
        assert_eq!(
            remote_services_map.get("api/v1/api.proto"),
            Some(&"custom+ApiService".to_string())
        );
    }

    #[test]
    fn test_options_string_building() {
        let manufacturer = "testco";
        let remote_paths = vec!["remote/s1.proto".to_string(), "remote/s2.proto".to_string()];
        let remote_actr_types = vec!["testco+S1".to_string(), "other+S2".to_string()];
        let local_paths = vec!["local.proto".to_string()];

        let mut options = format!("manufacturer={}", manufacturer);

        if !remote_paths.is_empty() {
            options.push_str(&format!(",RemoteFiles={}", remote_paths.join(":")));
            options.push_str(&format!(",RemoteActrTypes={}", remote_actr_types.join(":")));
        }

        if !local_paths.is_empty() {
            options.push_str(&format!(",LocalFiles={}", local_paths.join(":")));
            options.push_str(&format!(",LocalFile={}", local_paths[0]));
        }

        assert!(options.contains("manufacturer=testco"));
        assert!(options.contains("RemoteFiles=remote/s1.proto:remote/s2.proto"));
        assert!(options.contains("RemoteActrTypes=testco+S1:other+S2"));
        assert!(options.contains("LocalFiles=local.proto"));
        assert!(options.contains("LocalFile=local.proto"));
    }

    #[test]
    fn test_actr_type_extraction_logic() {
        let remote_services_map: HashMap<String, String> = [
            (
                "service1/api.proto".to_string(),
                "mfg1+Service1".to_string(),
            ),
            (
                "service2/api.proto".to_string(),
                "mfg2+Service2".to_string(),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        // Test matched path
        let path1 = "service1/api.proto";
        assert_eq!(
            remote_services_map.get(path1),
            Some(&"mfg1+Service1".to_string())
        );

        // Test unmatched path (should return None)
        let path2 = "unknown/api.proto";
        assert_eq!(remote_services_map.get(path2), None);

        // Test that we can handle None gracefully with empty string
        let actr_type = remote_services_map
            .get(path2)
            .cloned()
            .unwrap_or_else(|| String::new());
        assert_eq!(actr_type, "");
    }

    #[test]
    fn test_empty_lock_file_scenario() {
        // When lock file doesn't exist or has no dependencies
        let remote_services_map: HashMap<String, String> = HashMap::new();

        // Should handle gracefully
        assert_eq!(remote_services_map.len(), 0);
        assert_eq!(remote_services_map.get("any/path.proto"), None);

        // Simulating the warning path
        let path_str = "remote/service/api.proto";
        let is_in_map = remote_services_map.contains_key("service/api.proto");
        assert!(!is_in_map);
        // In actual code, this triggers warn! and pushes empty string
    }
}
