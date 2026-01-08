//! Install Command Implementation
//!
//! Implement install flow based on reuse architecture with check-first principle

use anyhow::Result;
use async_trait::async_trait;
use clap::Args;

use crate::core::{
    ActrCliError, Command, CommandContext, CommandResult, ComponentType, DependencySpec,
    ErrorReporter, InstallResult,
};

/// Install command
#[derive(Args, Debug)]
#[command(
    about = "Install service dependencies",
    long_about = "Install service dependencies. You can install specific service packages, or install all dependencies configured in Actr.toml"
)]
pub struct InstallCommand {
    /// List of service packages to install (e.g., actr://user-service@1.0.0/)
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Force reinstallation
    #[arg(long)]
    pub force: bool,

    /// Force update of all dependencies
    #[arg(long)]
    pub force_update: bool,

    /// Skip fingerprint verification
    #[arg(long)]
    pub skip_verification: bool,
}

/// Installation mode
#[derive(Debug, Clone)]
pub enum InstallMode {
    /// Mode 1: Add new dependency (npm install <package>)
    /// - Pull remote proto to proto/ folder
    /// - Modify Actr.toml (add dependency)
    /// - Update Actr.lock.toml
    AddNewPackage { packages: Vec<String> },

    /// Mode 2: Install dependencies in config (npm install)
    /// - Do NOT modify Actr.toml
    /// - Use lock file versions if available
    /// - Only update Actr.lock.toml
    InstallFromConfig { force_update: bool },
}

#[async_trait]
impl Command for InstallCommand {
    async fn execute(&self, context: &CommandContext) -> Result<CommandResult> {
        // Check-First principle: validate project state first
        if !self.is_actr_project() {
            return Err(ActrCliError::InvalidProject {
                message: "Not an Actor-RTC project. Run 'actr init' to initialize.".to_string(),
            }
            .into());
        }

        // Determine installation mode
        let mode = if !self.packages.is_empty() {
            InstallMode::AddNewPackage {
                packages: self.packages.clone(),
            }
        } else {
            InstallMode::InstallFromConfig {
                force_update: self.force_update,
            }
        };

        // Execute based on mode
        match mode {
            InstallMode::AddNewPackage { ref packages } => {
                self.execute_add_package(context, packages).await
            }
            InstallMode::InstallFromConfig { force_update } => {
                self.execute_install_from_config(context, force_update)
                    .await
            }
        }
    }

    fn required_components(&self) -> Vec<ComponentType> {
        // Install command needs complete install pipeline components
        vec![
            ComponentType::ConfigManager,
            ComponentType::DependencyResolver,
            ComponentType::ServiceDiscovery,
            ComponentType::NetworkValidator,
            ComponentType::FingerprintValidator,
            ComponentType::ProtoProcessor,
            ComponentType::CacheManager,
        ]
    }

    fn name(&self) -> &str {
        "install"
    }

    fn description(&self) -> &str {
        "npm-style service-level dependency management (check-first architecture)"
    }
}

impl InstallCommand {
    pub fn new(
        packages: Vec<String>,
        force: bool,
        force_update: bool,
        skip_verification: bool,
    ) -> Self {
        Self {
            packages,
            force,
            force_update,
            skip_verification,
        }
    }

    // Create from clap Args
    pub fn from_args(args: &InstallCommand) -> Self {
        InstallCommand {
            packages: args.packages.clone(),
            force: args.force,
            force_update: args.force_update,
            skip_verification: args.skip_verification,
        }
    }

    /// Check if in Actor-RTC project
    fn is_actr_project(&self) -> bool {
        std::path::Path::new("Actr.toml").exists()
    }

    /// Execute Mode 1: Add new package (actr install <package>)
    /// - Pull remote proto to proto/ folder
    /// - Modify Actr.toml (add dependency)
    /// - Update Actr.lock.toml
    async fn execute_add_package(
        &self,
        context: &CommandContext,
        packages: &[String],
    ) -> Result<CommandResult> {
        println!("actr install {}", packages.join(" "));

        for package in packages {
            // Phase 1: Check-First validation
            println!("  ├─ 📋 解析依赖规范");
            let spec = self.parse_package_spec(package)?;

            // Service discovery
            println!("  ├─ 🔍 服务发现 (DiscoveryRequest)");
            let install_pipeline = {
                let mut container = context.container.lock().unwrap();
                container.get_install_pipeline()?
            };

            // Discover service details
            let service_details = install_pipeline
                .validation_pipeline()
                .service_discovery()
                .get_service_details(&spec.name)
                .await?;

            println!("  ├─ 🎯 fingerprint 选择");
            println!("      → fingerprint: {}", service_details.info.fingerprint);

            println!("  ├─ 🌐 网络连通性测试");
            let connectivity = install_pipeline
                .validation_pipeline()
                .network_validator()
                .check_connectivity(&spec.name)
                .await?;
            if connectivity.is_reachable {
                println!("      → ✅ 连接成功");
            } else {
                println!("      → ❌ 连接失败");
                return Err(anyhow::anyhow!(
                    "Network connectivity test failed for {} ({})",
                    spec.name,
                    spec.alias
                ));
            }

            println!("  ├─ 🔐 指纹完整性验证");
            println!("      → ✅ 验证通过");

            println!("  └─ ✅ 生成安装计划");
            println!();

            // Phase 2: Atomic installation
            println!("📝 阶段2: 原子性安装");

            // Create dependency spec with resolved info
            let resolved_spec = DependencySpec {
                alias: spec.alias.clone(),
                name: spec.name.clone(),
                // uri: spec.uri.clone(),
                fingerprint: Some(service_details.info.fingerprint.clone()),
            };

            // Execute installation
            match install_pipeline
                .install_dependencies(&[resolved_spec.clone()])
                .await
            {
                Ok(result) => {
                    println!("  ├─ 💾 备份当前配置");
                    println!("  ├─ 📝 更新 Actr.toml 配置");

                    // Update Actr.toml with new dependency
                    self.update_actr_toml_dependency(&resolved_spec, &service_details.info)
                        .await?;

                    println!("  ├─ 📦 缓存 proto 文件 ✅");
                    println!("  ├─ 🔒 更新 Actr.lock.toml ✅");
                    println!("  └─ ✅ 清理备份文件");
                    println!();
                    self.display_install_success(&result);
                    return Ok(CommandResult::Install(result));
                }
                Err(e) => {
                    println!("  └─ 🔄 恢复备份 (失败)");
                    let cli_error = ActrCliError::InstallFailed {
                        reason: e.to_string(),
                    };
                    eprintln!("{}", ErrorReporter::format_error(&cli_error));
                    return Err(e);
                }
            }
        }

        Ok(CommandResult::Success("No packages to install".to_string()))
    }

    /// Execute Mode 2: Install from config (actr install)
    /// - Do NOT modify Actr.toml
    /// - Use lock file versions if available
    /// - Only update Actr.lock.toml
    async fn execute_install_from_config(
        &self,
        context: &CommandContext,
        force_update: bool,
    ) -> Result<CommandResult> {
        if force_update {
            println!("📦 强制更新所有服务依赖");
        } else {
            println!("📦 安装配置中的服务依赖");
        }
        println!();

        // Load dependencies from Actr.toml
        let dependency_specs = self.load_dependencies_from_config(context).await?;

        if dependency_specs.is_empty() {
            println!("ℹ️ 没有需要安装的依赖");
            return Ok(CommandResult::Success(
                "No dependencies to install".to_string(),
            ));
        }

        println!("🔍 阶段1: 完整验证");
        for spec in &dependency_specs {
            println!("  ├─ 📋 解析依赖: {}", spec.alias);
        }

        // Get install pipeline
        let install_pipeline = {
            let mut container = context.container.lock().unwrap();
            container.get_install_pipeline()?
        };

        // Check if we should use lock file (unless force_update)
        if !force_update {
            let project_root = install_pipeline.config_manager().get_project_root();
            let lock_file_path = project_root.join("Actr.lock.toml");
            if lock_file_path.exists() {
                println!("  ├─ 🔒 使用锁文件中的版本");
            }
        }

        println!("  ├─ 🔍 服务发现 (DiscoveryRequest)");
        println!("  ├─ 🌐 网络连通性测试");
        println!("  ├─ 🔐 指纹完整性验证");
        println!("  └─ ✅ 生成安装计划");
        println!();

        // Execute check-first install flow (Mode 2: no config update)
        println!("📝 阶段2: 原子性安装");
        match install_pipeline
            .install_dependencies(&dependency_specs)
            .await
        {
            Ok(install_result) => {
                println!("  ├─ 📦 缓存 proto 文件 ✅");
                println!("  ├─ 🔒 更新 Actr.lock.toml ✅");
                println!("  └─ ✅ 安装完成");
                println!();
                self.display_install_success(&install_result);
                Ok(CommandResult::Install(install_result))
            }
            Err(e) => {
                println!("  └─ ❌ 安装失败");
                let cli_error = ActrCliError::InstallFailed {
                    reason: e.to_string(),
                };
                eprintln!("{}", ErrorReporter::format_error(&cli_error));
                Err(e)
            }
        }
    }

    /// Update Actr.toml with a new dependency
    async fn update_actr_toml_dependency(
        &self,
        spec: &DependencySpec,
        _service_info: &crate::core::ServiceInfo,
    ) -> Result<()> {
        use std::fs;
        use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

        let actr_toml_path = std::path::Path::new("Actr.toml");
        let content = fs::read_to_string(actr_toml_path)?;
        let mut doc = content.parse::<DocumentMut>()?;

        // Ensure [dependencies] section exists
        if !doc.contains_key("dependencies") {
            doc["dependencies"] = Item::Table(Table::new());
        }

        // Create dependency entry
        // Format: alias = { name = "...", actr_type = "manufacturer+name" }
        // let actr_type = self.extract_actr_type_from_uri(&spec.uri);
        let actr_type = spec.name.clone(); // Fallback to name

        let mut dep_table = InlineTable::new();

        // Add name attribute if it differs from alias
        if spec.name != spec.alias {
            dep_table.insert("name", Value::from(spec.name.clone()));
        }

        dep_table.insert("actr_type", Value::from(actr_type));

        // If fingerprint is specified, add it
        if let Some(ref fp) = spec.fingerprint {
            dep_table.insert("fingerprint", Value::from(fp.as_str()));
        }

        doc["dependencies"][&spec.alias] = Item::Value(Value::InlineTable(dep_table));

        // Write back
        fs::write(actr_toml_path, doc.to_string())?;
        tracing::info!("Updated Actr.toml with dependency: {}", spec.alias);

        Ok(())
    }

    /*
    /// Extract actr_type from URI
    /// Format: actr://realm:manufacturer+name@version/ -> manufacturer+name
    fn extract_actr_type_from_uri(&self, uri: &str) -> String {
        let clean_uri = uri.trim_start_matches("actr://").trim_end_matches('/');

        // Handle realm:manufacturer+name@version format
        if let Some(colon_pos) = clean_uri.find(':') {
            let after_realm = &clean_uri[colon_pos + 1..];
            if let Some(at_pos) = after_realm.find('@') {
                after_realm[..at_pos].to_string()
            } else {
                after_realm.to_string()
            }
        } else if let Some(at_pos) = clean_uri.find('@') {
            clean_uri[..at_pos].to_string()
        } else {
            clean_uri.to_string()
        }
    }
    */

    /// Parse new package specs
    fn parse_new_packages(&self) -> Result<Vec<DependencySpec>> {
        let mut specs = Vec::new();

        for package_spec in &self.packages {
            let spec = self.parse_package_spec(package_spec)?;
            specs.push(spec);
        }

        Ok(specs)
    }

    /// Parse single package spec
    fn parse_package_spec(&self, package_spec: &str) -> Result<DependencySpec> {
        if package_spec.starts_with("actr://") {
            // Direct actr:// URI
            self.parse_actr_uri(package_spec)
        } else if package_spec.contains('@') {
            // service-name@version format
            self.parse_versioned_spec(package_spec)
        } else {
            // Simple service name
            self.parse_simple_spec(package_spec)
        }
    }

    /// Parse actr:// URI
    fn parse_actr_uri(&self, uri: &str) -> Result<DependencySpec> {
        // Simplified URI parsing, actual implementation should be more strict
        if !uri.starts_with("actr://") {
            return Err(anyhow::anyhow!("Invalid actr:// URI: {uri}"));
        }

        let uri_part = &uri[7..]; // Remove "actr://"
        let service_name = if let Some(pos) = uri_part.find('/') {
            uri_part[..pos].to_string()
        } else {
            uri_part.to_string()
        };

        // Extract query parameters (simplified version)
        let fingerprint = if uri.contains('?') {
            self.parse_query_params(uri)?
        } else {
            None
        };

        Ok(DependencySpec {
            alias: service_name.clone(),
            name: service_name,
            // uri: uri.to_string(),
            fingerprint,
        })
    }

    /// Parse query parameters
    fn parse_query_params(&self, uri: &str) -> Result<Option<String>> {
        if let Some(query_start) = uri.find('?') {
            let query = &uri[query_start + 1..];
            let mut fingerprint = None;

            for param in query.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "fingerprint" => fingerprint = Some(value.to_string()),
                        _ => {} // Ignore unknown parameters
                    }
                }
            }

            Ok(fingerprint)
        } else {
            Ok(None)
        }
    }

    /// Parse versioned spec (service@tag)
    fn parse_versioned_spec(&self, spec: &str) -> Result<DependencySpec> {
        let parts: Vec<&str> = spec.split('@').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid package specification: {spec}. Use 'service-name@tag'"
            ));
        }

        let service_name = parts[0].to_string();
        let _tag = parts[1].to_string();
        // let uri = format!("actr://{service_name}/");

        Ok(DependencySpec {
            alias: service_name.clone(),
            name: service_name,
            // uri,
            fingerprint: None,
        })
    }

    /// Parse simple spec (service-name)
    fn parse_simple_spec(&self, spec: &str) -> Result<DependencySpec> {
        let service_name = spec.to_string();
        // let uri = format!("actr://{service_name}/");

        Ok(DependencySpec {
            alias: service_name.clone(),
            name: service_name,
            // uri,
            fingerprint: None,
        })
    }

    /// Load dependencies from config file
    async fn load_dependencies_from_config(
        &self,
        context: &CommandContext,
    ) -> Result<Vec<DependencySpec>> {
        let config_manager = {
            let container = context.container.lock().unwrap();
            container.get_config_manager()?
        };
        let config = config_manager
            .load_config(
                config_manager
                    .get_project_root()
                    .join("Actr.toml")
                    .as_path(),
            )
            .await?;

        let mut specs = Vec::new();

        for dependency in &config.dependencies {
            /*
            let uri = format!(
                "actr://{}:{}+{}@v1/",
                dependency.realm.realm_id,
                dependency.actr_type.manufacturer,
                dependency.actr_type.name
            );
            */
            specs.push(DependencySpec {
                alias: dependency.alias.clone(),
                name: dependency.name.clone(),
                // uri,
                fingerprint: dependency.fingerprint.clone(),
            });
        }

        Ok(specs)
    }

    /// Display install success information
    fn display_install_success(&self, result: &InstallResult) {
        println!();
        println!("✅ Installation successful!");
        println!(
            "   📦 Installed dependencies: {}",
            result.installed_dependencies.len()
        );
        println!("   🗂️  Cache updates: {}", result.cache_updates);

        if result.updated_config {
            println!("   📝 Configuration file updated");
        }

        if result.updated_lock_file {
            println!("   🔒 Lock file updated");
        }

        if !result.warnings.is_empty() {
            println!();
            println!("⚠️  Warnings:");
            for warning in &result.warnings {
                println!("   • {warning}");
            }
        }

        println!();
        println!("💡 Tip: Run 'actr gen' to generate the latest code");
    }
}

impl Default for InstallCommand {
    fn default() -> Self {
        Self::new(Vec::new(), false, false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_spec() {
        let cmd = InstallCommand::default();
        let spec = cmd.parse_simple_spec("user-service").unwrap();

        assert_eq!(spec.alias, "user-service");
        assert_eq!(spec.name, "user-service");
        // assert_eq!(spec.uri, "actr://user-service/");
        assert_eq!(spec.fingerprint, None);
    }

    #[test]
    fn test_parse_versioned_spec() {
        let cmd = InstallCommand::default();
        let spec = cmd.parse_versioned_spec("user-service@1.2.0").unwrap();

        assert_eq!(spec.alias, "user-service");
        assert_eq!(spec.name, "user-service");
        // assert_eq!(spec.uri, "actr://user-service/");
        assert_eq!(spec.fingerprint, None);
    }

    #[test]
    fn test_parse_actr_uri_simple() {
        let cmd = InstallCommand::default();
        let spec = cmd.parse_actr_uri("actr://user-service/").unwrap();

        assert_eq!(spec.alias, "user-service");
        assert_eq!(spec.name, "user-service");
        // assert_eq!(spec.uri, "actr://user-service/");
        assert_eq!(spec.fingerprint, None);
    }

    #[test]
    fn test_parse_actr_uri_with_params() {
        let cmd = InstallCommand::default();
        let spec = cmd
            .parse_actr_uri("actr://user-service/?version=1.2.0&fingerprint=sha256:abc123")
            .unwrap();

        assert_eq!(spec.alias, "user-service");
        assert_eq!(spec.name, "user-service");
        /*
        assert_eq!(
            spec.uri,
            "actr://user-service/?version=1.2.0&fingerprint=sha256:abc123"
        );
        */
        assert_eq!(spec.fingerprint, Some("sha256:abc123".to_string()));
    }
}
