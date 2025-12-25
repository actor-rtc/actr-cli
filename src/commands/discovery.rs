//! Discovery Command Implementation
//!
//! Demonstrates multi-level reuse patterns: Service Discovery -> Validation -> Optional Install

use anyhow::Result;
use async_trait::async_trait;
use clap::Args;

use crate::core::{
    ActrCliError, Command, CommandContext, CommandResult, ComponentType, DependencySpec,
    ServiceInfo,
};

/// Discovery 命令
#[derive(Args, Debug)]
#[command(
    about = "Discover network services",
    long_about = "Discover Actor services in the network, view available services and choose to install"
)]
pub struct DiscoveryCommand {
    /// Service name filter pattern (e.g., user-*)
    #[arg(long, value_name = "PATTERN")]
    pub filter: Option<String>,

    /// Show detailed information
    #[arg(long)]
    pub verbose: bool,

    /// Automatically install selected services
    #[arg(long)]
    pub auto_install: bool,
}

#[async_trait]
impl Command for DiscoveryCommand {
    async fn execute(&self, context: &CommandContext) -> Result<CommandResult> {
        // Get reusable components
        let (service_discovery, user_interface, _config_manager) = {
            let container = context.container.lock().unwrap();
            (
                container.get_service_discovery()?,
                container.get_user_interface()?,
                container.get_config_manager()?,
            )
        };

        // Phase 1: Service Discovery
        println!("🔍 Scanning for Actor services in the network...");

        let filter = self.create_service_filter();
        let services = service_discovery.discover_services(filter.as_ref()).await?;

        if services.is_empty() {
            println!("ℹ️ No available Actor services discovered in the current network");
            return Ok(CommandResult::Success("No services discovered".to_string()));
        }

        // Display discovered services
        self.display_services_table(&services);

        // Phase 2: User Interaction Selection
        let selected_index = user_interface
            .select_service_from_list(&services, |s| format!("{} ({})", s.name, s.version))
            .await?;

        let selected_service = &services[selected_index];

        // Display service details and action menu
        self.display_service_details(selected_service).await?;

        // Ask user for action
        let action_menu = vec![
            "View service details".to_string(),
            "Export proto files".to_string(),
            "Add to configuration file".to_string(),
        ];

        let action_choice = user_interface
            .select_string_from_list(&action_menu, |s| s.clone())
            .await?;

        match action_choice {
            0 => {
                // View details
                self.show_detailed_service_info(selected_service, &service_discovery)
                    .await?;
                Ok(CommandResult::Success(
                    "Service details displayed".to_string(),
                ))
            }
            1 => {
                // Export proto files
                self.export_proto_files(selected_service, &service_discovery)
                    .await?;
                Ok(CommandResult::Success("Proto files exported".to_string()))
            }
            2 => {
                // Add to configuration file - core flow of reuse architecture
                self.add_to_config_with_validation(selected_service, context)
                    .await
            }
            _ => Ok(CommandResult::Success("Invalid choice".to_string())),
        }
    }

    fn required_components(&self) -> Vec<ComponentType> {
        // Components needed for Discovery command (supports complete reuse flow)
        vec![
            ComponentType::ServiceDiscovery,     // Core service discovery
            ComponentType::UserInterface,        // User interface
            ComponentType::ConfigManager,        // Configuration management
            ComponentType::DependencyResolver,   // Dependency resolution (validation phase)
            ComponentType::NetworkValidator,     // Network validation (validation phase)
            ComponentType::FingerprintValidator, // Fingerprint validation (validation phase)
            ComponentType::CacheManager,         // Cache management (install phase)
            ComponentType::ProtoProcessor,       // Proto processing (install phase)
        ]
    }

    fn name(&self) -> &str {
        "discovery"
    }

    fn description(&self) -> &str {
        "Discover available Actor services in the network (Reuse architecture + check-first)"
    }
}

impl DiscoveryCommand {
    pub fn new(filter: Option<String>, verbose: bool, auto_install: bool) -> Self {
        Self {
            filter,
            verbose,
            auto_install,
        }
    }

    // Create from clap Args
    pub fn from_args(args: &DiscoveryCommand) -> Self {
        DiscoveryCommand {
            filter: args.filter.clone(),
            verbose: args.verbose,
            auto_install: args.auto_install,
        }
    }

    /// Create service filter
    fn create_service_filter(&self) -> Option<crate::core::ServiceFilter> {
        self.filter
            .as_ref()
            .map(|pattern| crate::core::ServiceFilter {
                name_pattern: Some(pattern.clone()),
                version_range: None,
                tags: None,
            })
    }

    /// Display services table
    fn display_services_table(&self, services: &[ServiceInfo]) {
        println!();
        println!("🔍 Discovered Actor services:");
        println!();
        println!("┌─────────────────┬─────────┬─────────────────────────────────┐");
        println!("│ Service Name    │ Version │ Description                     │");
        println!("├─────────────────┼─────────┼─────────────────────────────────┤");

        for service in services {
            let description = service
                .description
                .as_deref()
                .unwrap_or("No description")
                .chars()
                .take(28)
                .collect::<String>();

            println!(
                "│ {:15} │ {:7} │ {:31} │",
                service.name.chars().take(15).collect::<String>(),
                service.version.chars().take(7).collect::<String>(),
                description
            );
        }

        println!("└─────────────────┴─────────┴─────────────────────────────────┘");
        println!();
        println!("→ Use ↑↓ to select service, Enter to view options, q to quit");
        println!();
    }

    /// Display service details
    async fn display_service_details(&self, service: &ServiceInfo) -> Result<()> {
        println!(
            "📋 Selected service: {} ({})",
            service.name, service.version
        );
        if let Some(desc) = &service.description {
            println!("📝 Description: {desc}");
        }
        println!("🔗 URI: {}", service.uri);
        println!("🔐 Fingerprint: {}", service.fingerprint);
        println!("📊 Methods count: {}", service.methods.len());
        println!();
        Ok(())
    }

    /// Show detailed service information
    async fn show_detailed_service_info(
        &self,
        service: &ServiceInfo,
        service_discovery: &std::sync::Arc<dyn crate::core::ServiceDiscovery>,
    ) -> Result<()> {
        println!("📖 {} Detailed Information:", service.name);
        println!("════════════════════════════════════════");

        let details = service_discovery.get_service_details(&service.uri).await?;

        println!("🏷️  Service Name: {}", details.info.name);
        println!("📦 Version: {}", details.info.version);
        println!("🔗 URI: {}", details.info.uri);
        println!("🔐 Fingerprint: {}", details.info.fingerprint);

        if let Some(desc) = &details.info.description {
            println!("📝 Description: {desc}");
        }

        println!();
        println!("📋 Available Methods:");
        for method in &details.info.methods {
            println!(
                "  • {}: {} → {}",
                method.name, method.input_type, method.output_type
            );
        }

        if !details.dependencies.is_empty() {
            println!();
            println!("🔗 Dependent Services:");
            for dep in &details.dependencies {
                println!("  • {dep}");
            }
        }

        println!();
        println!("📁 Proto Files:");
        for proto in &details.proto_files {
            println!("  • {} ({} services)", proto.name, proto.services.len());
        }

        Ok(())
    }

    /// Export proto files
    async fn export_proto_files(
        &self,
        service: &ServiceInfo,
        service_discovery: &std::sync::Arc<dyn crate::core::ServiceDiscovery>,
    ) -> Result<()> {
        println!("📤 Exporting proto files for {}...", service.name);

        let proto_files = service_discovery.get_service_proto(&service.uri).await?;

        for proto in &proto_files {
            let file_path = format!("./exported_{}", proto.name);
            std::fs::write(&file_path, &proto.content)?;
            println!("✅ Exported: {file_path}");
        }

        println!("🎉 Export completed, total {} files", proto_files.len());
        Ok(())
    }

    /// Add to configuration file - core flow of reuse architecture
    async fn add_to_config_with_validation(
        &self,
        service: &ServiceInfo,
        context: &CommandContext,
    ) -> Result<CommandResult> {
        let (config_manager, user_interface) = {
            let container = context.container.lock().unwrap();
            (
                container.get_config_manager()?,
                container.get_user_interface()?,
            )
        };

        // Convert to dependency spec
        let dependency_spec = DependencySpec {
            name: service.name.clone(),
            uri: service.uri.clone(),
            version: Some(service.version.clone()),
            fingerprint: Some(service.fingerprint.clone()),
        };

        println!("📝 Adding {} to configuration file...", service.name);

        // Backup configuration
        let backup = config_manager.backup_config().await?;

        // Update configuration
        match config_manager.update_dependency(&dependency_spec).await {
            Ok(_) => {
                println!("✅ Added {} to configuration file", service.name);
            }
            Err(e) => {
                config_manager.restore_backup(backup).await?;
                return Err(ActrCliError::Config {
                    message: format!("Configuration update failed: {e}"),
                }
                .into());
            }
        }

        // 🔍 复用 check 流程验证新依赖
        println!();
        println!("🔍 Verifying new dependency...");

        let validation_pipeline = {
            let mut container = context.container.lock().unwrap();
            container.get_validation_pipeline()?
        };

        match validation_pipeline
            .validate_dependencies(std::slice::from_ref(&dependency_spec))
            .await
        {
            Ok(validation_results) => {
                let all_passed = validation_results.iter().all(|v| v.is_available);

                if !all_passed {
                    // Verification failed, rollback configuration
                    println!(
                        "❌ Dependency verification failed, rolling back configuration changes..."
                    );
                    config_manager.restore_backup(backup).await?;

                    // Show detailed verification failure information
                    for validation in &validation_results {
                        if !validation.is_available {
                            println!(
                                "  • {}: {}",
                                validation.dependency,
                                validation.error.as_deref().unwrap_or("Verification failed")
                            );
                        }
                    }

                    return Err(ActrCliError::ValidationFailed {
                        details: "Dependency verification failed".to_string(),
                    }
                    .into());
                } else {
                    // Verification successful
                    println!("  ├─ 📋 Service existence check ✅");
                    println!("  ├─ 🌐 Network connectivity test ✅");
                    println!("  └─ 🔐 Fingerprint integrity verification ✅");

                    // Clean up backup
                    config_manager.remove_backup(backup).await?;
                }
            }
            Err(e) => {
                // Verification error, rollback configuration
                println!("❌ Error during verification, rolling back configuration changes...");
                config_manager.restore_backup(backup).await?;
                return Err(e);
            }
        }

        // Ask if user wants to install immediately
        println!();
        let should_install = if self.auto_install {
            true
        } else {
            user_interface
                .confirm("🤔 Install this dependency now?")
                .await?
        };

        if should_install {
            // Reuse install flow
            println!();
            println!("📦 Installing {}...", service.name);

            let install_pipeline = {
                let mut container = context.container.lock().unwrap();
                container.get_install_pipeline()?
            };

            match install_pipeline
                .install_dependencies(&[dependency_spec])
                .await
            {
                Ok(install_result) => {
                    println!("  ├─ 📦 Cache proto files ✅");
                    println!("  ├─ 🔒 Update lock file ✅");
                    println!("  └─ ✅ Installation complete");
                    println!();
                    println!("💡 Tip: Run 'actr gen' to generate the latest code");

                    Ok(CommandResult::Install(install_result))
                }
                Err(e) => {
                    eprintln!("❌ Installation failed: {e}");
                    Ok(CommandResult::Success(
                        "Dependency added but installation failed".to_string(),
                    ))
                }
            }
        } else {
            println!("✅ Dependency added to configuration file");
            println!("💡 Tip: Run 'actr install' to install dependencies");
            Ok(CommandResult::Success(
                "Dependency added to configuration".to_string(),
            ))
        }
    }
}

impl Default for DiscoveryCommand {
    fn default() -> Self {
        Self::new(None, false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_service_filter() {
        let cmd = DiscoveryCommand::new(Some("user-*".to_string()), false, false);
        let filter = cmd.create_service_filter();

        assert!(filter.is_some());
        let filter = filter.unwrap();
        assert_eq!(filter.name_pattern, Some("user-*".to_string()));
    }

    #[test]
    fn test_create_service_filter_none() {
        let cmd = DiscoveryCommand::new(None, false, false);
        let filter = cmd.create_service_filter();

        assert!(filter.is_none());
    }

    #[test]
    fn test_required_components() {
        let cmd = DiscoveryCommand::default();
        let components = cmd.required_components();

        // Discovery command needs to support complete reuse flow
        assert!(components.contains(&ComponentType::ServiceDiscovery));
        assert!(components.contains(&ComponentType::UserInterface));
        assert!(components.contains(&ComponentType::ConfigManager));
        assert!(components.contains(&ComponentType::DependencyResolver));
        assert!(components.contains(&ComponentType::NetworkValidator));
        assert!(components.contains(&ComponentType::FingerprintValidator));
        assert!(components.contains(&ComponentType::CacheManager));
        assert!(components.contains(&ComponentType::ProtoProcessor));
    }
}
