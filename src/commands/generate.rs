//! # Code Generation Command
//!
//! Generate Rust Actor code from proto files, including:
//! 1. Protobuf message types
//! 2. Actor infrastructure code
//! 3. User business logic scaffolds (with TODO comments)

use crate::commands::Command;
use crate::commands::SupportedLanguage;
use crate::commands::codegen::{GenContext, execute_codegen};
use crate::error::{ActrCliError, Result};
use crate::utils::to_pascal_case;
// 只导入必要的类型，避免拉入不需要的依赖如 sqlite
// use actr_framework::prelude::*;
use async_trait::async_trait;
use clap::Args;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tracing::{debug, info, warn};

#[derive(Args, Debug, Clone)]
#[command(
    about = "Generate code from proto files",
    after_help = "Default output paths by language:
  - rust:   src/generated
  - swift:  {PascalName}/Generated (e.g., EchoApp/Generated)
  - kotlin: app/src/main/java/{package}/generated
  - python: generated"
)]
pub struct GenCommand {
    /// Input proto file or directory
    #[arg(short, long, default_value = "protos")]
    pub input: PathBuf,

    /// Output directory for generated code (use -o to override language defaults)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Path to Actr.toml config file
    #[arg(short, long, default_value = "Actr.toml")]
    pub config: PathBuf,

    /// Clean generated outputs before regenerating
    #[arg(long = "clean")]
    pub clean: bool,

    /// Skip user code scaffold generation
    #[arg(long = "no-scaffold")]
    pub no_scaffold: bool,

    /// Whether to overwrite existing user code files
    #[arg(long)]
    pub overwrite_user_code: bool,

    /// Skip formatting
    #[arg(long = "no-format")]
    pub no_format: bool,

    /// Debug mode: keep intermediate generated files
    #[arg(long)]
    pub debug: bool,

    /// Target language for generation
    #[arg(short, long, default_value = "rust")]
    pub language: SupportedLanguage,
}

#[async_trait]
impl Command for GenCommand {
    async fn execute(&self) -> Result<()> {
        // Check if Actr.lock.toml exists
        self.check_lock_file()?;

        // Determine output path based on language
        let output = self.determine_output_path()?;

        info!(
            "🚀 Start code generation (language: {:?})...",
            self.language
        );
        let config = actr_config::ConfigParser::from_file(&self.config)
            .map_err(|e| ActrCliError::config_error(format!("Failed to parse Actr.toml: {e}")))?;

        let proto_files = self.preprocess()?;
        if self.language != SupportedLanguage::Rust {
            let context = GenContext {
                proto_files,
                input_path: self.input.clone(),
                output,
                config: config.clone(),
                no_scaffold: self.no_scaffold,
                overwrite_user_code: self.overwrite_user_code,
                no_format: self.no_format,
                debug: self.debug,
            };
            execute_codegen(self.language, &context).await?;
            return Ok(());
        }

        // Step 5: Generate infrastructure code
        self.generate_infrastructure_code(&proto_files, &config)
            .await?;

        // Step 6: Generate user code scaffold
        if self.should_generate_scaffold() {
            self.generate_user_code_scaffold(&proto_files).await?;
        }

        // Step 7: Format code
        if self.should_format() {
            self.format_generated_code().await?;
        }

        // Step 8: Validate generated code
        self.validate_generated_code().await?;

        info!("✅ Code generation completed!");
        // Set all generated files to read-only only after generation, formatting, and validation are complete, to not interfere with rustfmt or other steps.
        self.set_generated_files_readonly()?;
        self.print_next_steps();

        Ok(())
    }
}

impl GenCommand {
    /// Check if Actr.lock.toml exists and provide helpful error message if not
    fn check_lock_file(&self) -> Result<()> {
        let config_dir = self
            .config
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let lock_file_path = config_dir.join("Actr.lock.toml");

        if !lock_file_path.exists() {
            return Err(ActrCliError::config_error(
                "Actr.lock.toml not found\n\n\
                The lock file is required for code generation. Please run:\n\n\
                \x20\x20\x20\x20actr install\n\n\
                This will generate Actr.lock.toml based on your Actr.toml configuration.",
            ));
        }

        Ok(())
    }

    /// Determine output path based on language if not explicitly specified
    fn determine_output_path(&self) -> Result<PathBuf> {
        // If user specified a custom output, use it
        if let Some(ref output) = self.output {
            return Ok(output.clone());
        }

        // Determine language-specific default output path
        match self.language {
            SupportedLanguage::Swift => {
                // Read package name from config for Swift
                let config = actr_config::ConfigParser::from_file(&self.config).map_err(|e| {
                    ActrCliError::config_error(format!("Failed to parse Actr.toml: {e}"))
                })?;
                let project_name = &config.package.name;
                // Convert to PascalCase for Swift module name
                let pascal_name = to_pascal_case(project_name);
                Ok(PathBuf::from(format!("{}/Generated", pascal_name)))
            }
            SupportedLanguage::Kotlin => {
                // Kotlin default: app/src/main/java/{package_path}/generated
                let config = actr_config::ConfigParser::from_file(&self.config).map_err(|e| {
                    ActrCliError::config_error(format!("Failed to parse Actr.toml: {e}"))
                })?;
                let package_name = config.package.name.replace("-", ".");
                let package_path = package_name.replace(".", "/");
                Ok(PathBuf::from(format!(
                    "app/src/main/java/{}/generated",
                    package_path
                )))
            }
            SupportedLanguage::Python => {
                // Python default: generated
                Ok(PathBuf::from("generated"))
            }
            SupportedLanguage::Rust => {
                // Rust default: src/generated
                Ok(PathBuf::from("src/generated"))
            }
        }
    }

    fn preprocess(&self) -> Result<Vec<PathBuf>> {
        // Step 1: Validate inputs
        self.validate_inputs()?;

        // Step 2: Clean old generation outputs (optional)
        self.clean_generated_outputs()?;

        // Step 3: Prepare output directories
        self.prepare_output_dirs()?;

        // Step 4: Discover proto files
        let proto_files = self.discover_proto_files()?;
        info!("📁 Found {} proto files", proto_files.len());

        Ok(proto_files)
    }

    /// Whether user code scaffold should be generated
    fn should_generate_scaffold(&self) -> bool {
        !self.no_scaffold
    }

    /// Whether formatting should run
    fn should_format(&self) -> bool {
        !self.no_format
    }

    /// Remove previously generated files when --clean is used
    fn clean_generated_outputs(&self) -> Result<()> {
        use std::fs;

        if !self.clean {
            return Ok(());
        }

        let output = self.determine_output_path()?;
        if !output.exists() {
            return Ok(());
        }

        info!("🧹 Cleaning old generation results: {:?}", output);

        self.make_writable_recursive(&output)?;
        fs::remove_dir_all(&output).map_err(|e| {
            ActrCliError::config_error(format!("Failed to delete generation directory: {e}"))
        })?;

        Ok(())
    }

    /// Ensure all files are writable so removal works across platforms
    #[allow(clippy::only_used_in_recursion)]
    fn make_writable_recursive(&self, path: &Path) -> Result<()> {
        use std::fs;

        if path.is_file() {
            let metadata = fs::metadata(path).map_err(|e| {
                ActrCliError::config_error(format!("Failed to read file metadata: {e}"))
            })?;
            let mut permissions = metadata.permissions();

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = permissions.mode();
                permissions.set_mode(mode | 0o222);
            }

            #[cfg(not(unix))]
            {
                permissions.set_readonly(false);
            }

            fs::set_permissions(path, permissions).map_err(|e| {
                ActrCliError::config_error(format!("Failed to reset file permissions: {e}"))
            })?;
        } else if path.is_dir() {
            for entry in fs::read_dir(path)
                .map_err(|e| ActrCliError::config_error(format!("Failed to read directory: {e}")))?
            {
                let entry = entry.map_err(|e| ActrCliError::config_error(e.to_string()))?;
                self.make_writable_recursive(&entry.path())?;
            }
        }

        Ok(())
    }

    /// 验证输入参数
    fn validate_inputs(&self) -> Result<()> {
        if !self.input.exists() {
            return Err(ActrCliError::config_error(format!(
                "Input path does not exist: {:?}",
                self.input
            )));
        }

        if self.input.is_file() && self.input.extension().unwrap_or_default() != "proto" {
            warn!("Input file is not a .proto file: {:?}", self.input);
        }

        Ok(())
    }

    /// 准备输出目录
    fn prepare_output_dirs(&self) -> Result<()> {
        let output = self.determine_output_path()?;
        std::fs::create_dir_all(&output).map_err(|e| {
            ActrCliError::config_error(format!("Failed to create output directory: {e}"))
        })?;

        if self.should_generate_scaffold() {
            let user_code_dir = output.join("../");
            std::fs::create_dir_all(&user_code_dir).map_err(|e| {
                ActrCliError::config_error(format!("Failed to create user code directory: {e}"))
            })?;
        }

        Ok(())
    }

    /// Find proto files recursively
    fn discover_proto_files(&self) -> Result<Vec<PathBuf>> {
        let mut proto_files = Vec::new();

        if self.input.is_file() {
            proto_files.push(self.input.clone());
        } else {
            self.collect_proto_files(&self.input, &mut proto_files)?;
        }

        if proto_files.is_empty() {
            return Err(ActrCliError::config_error("No proto files found"));
        }

        Ok(proto_files)
    }

    /// Collect proto files recursively
    #[allow(clippy::only_used_in_recursion)]
    fn collect_proto_files(&self, dir: &PathBuf, proto_files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir)
            .map_err(|e| ActrCliError::config_error(format!("Failed to read directory: {e}")))?
        {
            let entry = entry.map_err(|e| ActrCliError::config_error(e.to_string()))?;
            let path = entry.path();

            if path.is_file() && path.extension().unwrap_or_default() == "proto" {
                proto_files.push(path);
            } else if path.is_dir() {
                self.collect_proto_files(&path, proto_files)?;
            }
        }
        Ok(())
    }

    /// 确保 protoc-gen-actrframework 插件可用
    ///
    /// 版本管理策略：
    /// 1. 检查系统已安装版本
    /// 2. 如果版本匹配 → 直接使用
    /// 3. 如果版本不匹配或未安装 → 自动安装/升级
    ///
    /// 这种策略确保：
    /// - 版本一致性：插件版本始终与 CLI 匹配
    /// - 自动管理：无需手动安装或升级
    /// - 简单明确：只看版本，不区分开发/生产环境
    fn ensure_protoc_plugin(&self) -> Result<PathBuf> {
        // Expected version (same as actr-framework-protoc-codegen)
        const EXPECTED_VERSION: &str = env!("CARGO_PKG_VERSION");

        // 1. Check installed version
        let installed_version = self.check_installed_plugin_version()?;

        match installed_version {
            Some(version) if version == EXPECTED_VERSION => {
                // Version matches, use it directly
                info!("✅ Using installed protoc-gen-actrframework v{}", version);
                let output = StdCommand::new("which")
                    .arg("protoc-gen-actrframework")
                    .output()
                    .map_err(|e| {
                        ActrCliError::command_error(format!("Failed to locate plugin: {e}"))
                    })?;

                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(PathBuf::from(path))
            }
            Some(version) => {
                // Version mismatch, upgrade needed
                info!(
                    "🔄 Version mismatch: installed v{}, need v{}",
                    version, EXPECTED_VERSION
                );
                info!("🔨 Upgrading plugin...");
                self.install_or_upgrade_plugin()
            }
            None => {
                // Not installed, install it
                info!("📦 protoc-gen-actrframework not found, installing...");
                self.install_or_upgrade_plugin()
            }
        }
    }

    /// Check installed plugin version
    fn check_installed_plugin_version(&self) -> Result<Option<String>> {
        let output = StdCommand::new("protoc-gen-actrframework")
            .arg("--version")
            .output();

        match output {
            Ok(output) if output.status.success() => {
                let version_info = String::from_utf8_lossy(&output.stdout);
                // Parse "protoc-gen-actrframework 0.1.0"
                let version = version_info
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(|v| v.to_string());

                debug!("Detected installed version: {:?}", version);
                Ok(version)
            }
            _ => {
                debug!("Plugin not found in PATH");
                Ok(None)
            }
        }
    }

    /// Install or upgrade plugin from workspace
    fn install_or_upgrade_plugin(&self) -> Result<PathBuf> {
        // Find actr workspace
        let current_dir = std::env::current_dir()?;
        let workspace_root = current_dir.ancestors().find(|p| {
            let is_workspace =
                p.join("Cargo.toml").exists() && p.join("crates/framework-protoc-codegen").exists();
            if is_workspace {
                debug!("Found workspace root: {:?}", p);
            }
            is_workspace
        });

        let workspace_root = workspace_root.ok_or_else(|| {
            ActrCliError::config_error(
                "Cannot find actr workspace.\n\
                 Please run this command from within an actr project or workspace.",
            )
        })?;

        info!("🔍 Found actr workspace at: {}", workspace_root.display());

        // Step 1: Build the plugin
        info!("🔨 Building protoc-gen-actrframework...");
        let mut build_cmd = StdCommand::new("cargo");
        build_cmd
            .arg("build")
            .arg("-p")
            .arg("actr-framework-protoc-codegen")
            .arg("--bin")
            .arg("protoc-gen-actrframework")
            .current_dir(workspace_root);

        debug!("Running: {:?}", build_cmd);
        let output = build_cmd
            .output()
            .map_err(|e| ActrCliError::command_error(format!("Failed to build plugin: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ActrCliError::command_error(format!(
                "Failed to build plugin:\n{stderr}"
            )));
        }

        // Step 2: Install to ~/.cargo/bin/
        info!("📦 Installing to ~/.cargo/bin/...");
        let mut install_cmd = StdCommand::new("cargo");
        install_cmd
            .arg("install")
            .arg("--path")
            .arg(workspace_root.join("crates/framework-protoc-codegen"))
            .arg("--bin")
            .arg("protoc-gen-actrframework")
            .arg("--force"); // Overwrite existing version

        debug!("Running: {:?}", install_cmd);
        let output = install_cmd
            .output()
            .map_err(|e| ActrCliError::command_error(format!("Failed to install plugin: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ActrCliError::command_error(format!(
                "Failed to install plugin:\n{stderr}"
            )));
        }

        info!("✅ Plugin installed successfully");

        // Return the installed path
        let which_output = StdCommand::new("which")
            .arg("protoc-gen-actrframework")
            .output()
            .map_err(|e| {
                ActrCliError::command_error(format!("Failed to locate installed plugin: {e}"))
            })?;

        let path = String::from_utf8_lossy(&which_output.stdout)
            .trim()
            .to_string();
        Ok(PathBuf::from(path))
    }

    /// 生成基础设施代码
    async fn generate_infrastructure_code(
        &self,
        proto_files: &[PathBuf],
        config: &actr_config::Config,
    ) -> Result<()> {
        info!("🔧 Generating infrastructure code...");

        // 确保 protoc 插件可用
        let plugin_path = self.ensure_protoc_plugin()?;

        let manufacturer = config.package.actr_type.manufacturer.clone();
        debug!("Using manufacturer from Actr.toml: {}", manufacturer);

        // 确定输出路径
        let output = self.determine_output_path()?;

        for proto_file in proto_files {
            debug!("Processing proto file: {:?}", proto_file);

            // 第一步：使用 prost 生成基础 protobuf 消息类型
            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", self.input.display()))
                .arg("--prost_opt=flat_output_dir")
                .arg(format!("--prost_out={}", output.display()))
                .arg(proto_file);

            debug!("Executing protoc (prost): {:?}", cmd);
            let output_cmd = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!("Failed to execute protoc (prost): {e}"))
            })?;

            if !output_cmd.status.success() {
                let stderr = String::from_utf8_lossy(&output_cmd.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (prost) execution failed: {stderr}"
                )));
            }

            // 第二步：使用 actrframework 插件生成 Actor 框架代码
            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", self.input.display()))
                .arg(format!(
                    "--plugin=protoc-gen-actrframework={}",
                    plugin_path.display()
                ))
                .arg(format!("--actrframework_opt=manufacturer={manufacturer}"))
                .arg(format!("--actrframework_out={}", output.display()))
                .arg(proto_file);

            debug!("Executing protoc (actrframework): {:?}", cmd);
            let output_cmd = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!(
                    "Failed to execute protoc (actrframework): {e}"
                ))
            })?;

            if !output_cmd.status.success() {
                let stderr = String::from_utf8_lossy(&output_cmd.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (actrframework) execution failed: {stderr}"
                )));
            }

            let stdout = String::from_utf8_lossy(&output_cmd.stdout);
            if !stdout.is_empty() {
                debug!("protoc output: {}", stdout);
            }
        }

        // 生成 mod.rs
        self.generate_mod_rs(proto_files).await?;

        info!("✅ Infrastructure code generation completed");
        Ok(())
    }

    /// 生成 mod.rs 文件
    async fn generate_mod_rs(&self, _proto_files: &[PathBuf]) -> Result<()> {
        let output = self.determine_output_path()?;
        let mod_path = output.join("mod.rs");

        // 扫描实际生成的文件，而不是根据 proto 文件名猜测
        let mut proto_modules = Vec::new();
        let mut service_modules = Vec::new();

        use std::fs;
        for entry in fs::read_dir(&output).map_err(|e| {
            ActrCliError::config_error(format!("Failed to read output directory: {e}"))
        })? {
            let entry = entry.map_err(|e| ActrCliError::config_error(e.to_string()))?;
            let path = entry.path();

            if path.is_file()
                && path.extension().unwrap_or_default() == "rs"
                && let Some(file_name) = path.file_stem().and_then(|s| s.to_str())
            {
                // 跳过 mod.rs 本身
                if file_name == "mod" {
                    continue;
                }

                // 区分 service_actor 文件和 proto 文件
                if file_name.ends_with("_service_actor") {
                    service_modules.push(format!("pub mod {file_name};"));
                } else {
                    proto_modules.push(format!("pub mod {file_name};"));
                }
            }
        }

        // 排序以保证生成的 mod.rs 内容稳定
        proto_modules.sort();
        service_modules.sort();

        let mod_content = format!(
            r#"//! Automatically generated code module
//!
//! This module is automatically generated by the `actr gen` command, including:
//! - protobuf message type definitions
//! - Actor framework code (router, traits)
//!
//! ⚠️ Do not manually modify files in this directory

// Protobuf message types (generated by prost)
{}

// Actor framework code (generated by protoc-gen-actrframework)
{}

// Common types are defined in their respective modules, please import as needed
"#,
            proto_modules.join("\n"),
            service_modules.join("\n"),
        );

        std::fs::write(&mod_path, mod_content)
            .map_err(|e| ActrCliError::config_error(format!("Failed to write mod.rs: {e}")))?;

        debug!("Generated mod.rs: {:?}", mod_path);
        Ok(())
    }

    /// 将生成目录中的文件设置为只读
    fn set_generated_files_readonly(&self) -> Result<()> {
        use std::fs;

        let output = self.determine_output_path()?;
        for entry in fs::read_dir(&output).map_err(|e| {
            ActrCliError::config_error(format!("Failed to read output directory: {e}"))
        })? {
            let entry = entry.map_err(|e| ActrCliError::config_error(e.to_string()))?;
            let path = entry.path();

            if path.is_file() && path.extension().unwrap_or_default() == "rs" {
                // 获取当前权限
                let metadata = fs::metadata(&path).map_err(|e| {
                    ActrCliError::config_error(format!("Failed to get file metadata: {e}"))
                })?;
                let mut permissions = metadata.permissions();

                // 设置只读（移除写权限）
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = permissions.mode();
                    permissions.set_mode(mode & !0o222); // 移除所有写权限
                }

                #[cfg(not(unix))]
                {
                    permissions.set_readonly(true);
                }

                fs::set_permissions(&path, permissions).map_err(|e| {
                    ActrCliError::config_error(format!("Failed to set file permissions: {e}"))
                })?;

                debug!("Set read-only attribute: {:?}", path);
            }
        }

        Ok(())
    }

    /// Generate user code scaffold
    async fn generate_user_code_scaffold(&self, proto_files: &[PathBuf]) -> Result<()> {
        info!("📝 Generating user code scaffold...");

        for proto_file in proto_files {
            let service_name = proto_file
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| ActrCliError::config_error("Invalid proto file name"))?;

            self.generate_service_scaffold(service_name).await?;
        }

        info!("✅ User code scaffold generation completed");
        Ok(())
    }

    /// Generate scaffold for a specific service
    async fn generate_service_scaffold(&self, service_name: &str) -> Result<()> {
        let output = self.determine_output_path()?;
        let user_file_path = output
            .parent()
            .unwrap_or_else(|| Path::new("src"))
            .join(format!("{}_service.rs", service_name.to_lowercase()));

        // If file exists and overwrite is not forced, skip
        if user_file_path.exists() && !self.overwrite_user_code {
            info!("⏭️  Skipping existing user code file: {:?}", user_file_path);
            return Ok(());
        }

        let scaffold_content = self.generate_scaffold_content(service_name);

        std::fs::write(&user_file_path, scaffold_content).map_err(|e| {
            ActrCliError::config_error(format!("Failed to write user code scaffold: {e}"))
        })?;

        info!("📄 Generated user code scaffold: {:?}", user_file_path);
        Ok(())
    }

    /// 生成用户代码框架内容
    fn generate_scaffold_content(&self, service_name: &str) -> String {
        let service_name_pascal = service_name
            .split('_')
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<String>();

        let template = format!(
            r#"//! # {service_name_pascal} user business logic implementation
//!
//! This file is a user code scaffold automatically generated by the `actr gen` command.
//! Please implement your specific business logic here.

use crate::generated::{{{service_name_pascal}Handler, {service_name_pascal}Actor}};
// 只导入必要的类型，避免拉入不需要的依赖如 sqlite
// use actr_framework::prelude::*;
use std::sync::Arc;

/// Specific implementation of the {service_name_pascal} service
/// 
/// TODO: Add state fields you need, for example:
/// - Database connection pool
/// - Configuration information
/// - Cache client
/// - Logger, etc.
pub struct My{service_name_pascal}Service {{
    // TODO: Add your service state fields
    // For example:
    // pub db_pool: Arc<DatabasePool>,
    // pub config: Arc<ServiceConfig>,
    // pub metrics: Arc<Metrics>,
}}

impl My{service_name_pascal}Service {{
    /// Create a new service instance
    /// 
    /// TODO: Modify constructor parameters as needed
    pub fn new(/* TODO: Add necessary dependencies */) -> Self {{
        Self {{
            // TODO: Initialize your fields
        }}
    }}
    
    /// Create a service instance with default configuration (for testing)
    pub fn default_for_testing() -> Self {{
        Self {{
            // TODO: Provide default values for testing
        }}
    }}
}}

// TODO: Implement all methods of the {service_name_pascal}Handler trait
// Note: The impl_user_code_scaffold! macro has generated a basic scaffold for you,
// you need to replace it with real business logic implementation.
//
// Example:
// #[async_trait]
// impl {service_name_pascal}Handler for My{service_name_pascal}Service {{
//     async fn method_name(&self, req: RequestType) -> ActorResult<ResponseType> {{
//         // 1. Validate input
//         // 2. Execute business logic
//         // 3. Return result
//         todo!("Implement your business logic")
//     }}
// }}

#[cfg(test)]
mod tests {{
    use super::*;
    
    #[tokio::test]
    async fn test_service_creation() {{
        let _service = My{service_name_pascal}Service::default_for_testing();
        // TODO: Add your tests
    }}
    
    // TODO: Add more test cases
}}

/*
📚 User Guide

## 🚀 Quick Start

1. **Implement business logic**:
   Implement all methods of the `{service_name_pascal}Handler` trait in `My{service_name_pascal}Service`

2. **Add dependencies**:
   Add dependencies you need in `Cargo.toml`, such as database clients, HTTP clients, etc.

3. **Configure service**:
   Modify the `new()` constructor to inject necessary dependencies

4. **Start service**:
   ```rust
   #[tokio::main]
   async fn main() -> ActorResult<()> {{
       let service = My{service_name_pascal}Service::new(/* dependencies */);
       
       ActorSystem::new()
           .attach(service)
           .start()
           .await
   }}
   ```

## 🔧 Development Tips

- Use `tracing` crate for logging
- Implement error handling and retry logic
- Add unit and integration tests
- Consider using configuration files for environment variables
- Implement health checks and metrics collection

## 📖 More Resources

- Actor-RTC Documentation: [Link]
- API Reference: [Link]
- Example Projects: [Link]
*/
"# // Service in example code
        );

        template
    }

    /// 格式化生成的代码
    async fn format_generated_code(&self) -> Result<()> {
        info!("🎨 Formatting generated code...");

        let mut cmd = StdCommand::new("rustfmt");
        cmd.arg("--edition")
            .arg("2024")
            .arg("--config")
            .arg("max_width=100");

        // 格式化生成目录中的所有 .rs 文件
        let output = self.determine_output_path()?;
        for entry in std::fs::read_dir(&output).map_err(|e| {
            ActrCliError::config_error(format!("Failed to read output directory: {e}"))
        })? {
            let entry = entry.map_err(|e| ActrCliError::config_error(e.to_string()))?;
            let path = entry.path();

            if path.extension().unwrap_or_default() == "rs" {
                cmd.arg(&path);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| ActrCliError::command_error(format!("Failed to execute rustfmt: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("rustfmt execution warning: {}", stderr);
        } else {
            info!("✅ Code formatting completed");
        }

        Ok(())
    }

    /// 验证生成的代码
    async fn validate_generated_code(&self) -> Result<()> {
        info!("🔍 Validating generated code...");

        // 查找项目根目录（包含 Cargo.toml 的目录）
        let project_root = self.find_project_root()?;

        let mut cmd = StdCommand::new("cargo");
        cmd.arg("check").arg("--quiet").current_dir(&project_root);

        let output = cmd.output().map_err(|e| {
            ActrCliError::command_error(format!("Failed to execute cargo check: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                "Generated code has compilation warnings or errors:\n{}",
                stderr
            );
            info!("💡 This is usually normal because the user code scaffold contains TODO markers");
        } else {
            info!("✅ Code validation passed");
        }

        Ok(())
    }

    /// 查找项目根目录（包含 Cargo.toml 的目录）
    fn find_project_root(&self) -> Result<PathBuf> {
        let mut current = std::env::current_dir().map_err(ActrCliError::Io)?;

        loop {
            if current.join("Cargo.toml").exists() {
                return Ok(current);
            }

            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => break,
            }
        }

        // 如果找不到 Cargo.toml，回退到当前目录
        std::env::current_dir().map_err(ActrCliError::Io)
    }

    /// 打印后续步骤提示
    fn print_next_steps(&self) {
        println!("\n🎉 Code generation completed!");
        println!("\n📋 Next steps:");
        let output = self
            .determine_output_path()
            .unwrap_or_else(|_| PathBuf::from("src/generated"));
        println!("1. 📖 View generated code: {:?}", output);
        if self.should_generate_scaffold() {
            println!(
                "2. ✏️  Implement business logic: in the *_service.rs files in the src/ directory"
            );
            println!("3. 🔧 Add dependencies: add required packages in Cargo.toml");
            println!("4. 🏗️  Build project: cargo build");
            println!("5. 🧪 Run tests: cargo test");
            println!("6. 🚀 Start service: cargo run");
        } else {
            println!("2. 🏗️  Build project: cargo build");
            println!("3. 🧪 Run tests: cargo test");
            println!("4. 🚀 Start service: cargo run");
        }
        println!("\n💡 Tip: Check the detailed user guide in the generated user code files");
    }
}
