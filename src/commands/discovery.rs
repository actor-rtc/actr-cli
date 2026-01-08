//! Discovery Command Implementation
//!
//! Demonstrates multi-level reuse patterns: Service Discovery -> Validation -> Optional Install

use anyhow::Result;
use async_trait::async_trait;
use clap::Args;

use crate::core::{
    ActrCliError, Command, CommandContext, CommandResult, ComponentType, DependencySpec,
    ServiceDetails, ServiceInfo,
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

        let filter = self.create_service_filter();
        let services = service_discovery.discover_services(filter.as_ref()).await?;
        tracing::debug!("Discovered services: {:?}", services);

        if services.is_empty() {
            println!("ℹ️ No available Actor services discovered in the current network");
            return Ok(CommandResult::Success("No services discovered".to_string()));
        }

        println!("🔍 Discovered Actor services:");
        // Display discovered services table
        self.display_services_table(&services);

        // Selection Phase
        let service_options: Vec<String> = services.iter().map(|s| s.name.clone()).collect();

        let selected_index = user_interface
            .select_from_list(&service_options, "Select a service to view (Esc to quit)")
            .await?;

        let selected_service = &services[selected_index];

        // Action menu prompt
        let menu_prompt = format!("Options for {}", selected_service.name);

        // Action menu items (as shown in screenshot)
        let action_menu = vec![
            "[1] View service details (fingerprint, publication time)".to_string(),
            "[2] Export proto files".to_string(),
            "[3] Add to configuration file".to_string(),
        ];

        let action_choice = user_interface
            .select_from_list(&action_menu, &menu_prompt)
            .await?;

        match action_choice {
            0 => {
                self.display_service_info(&selected_service);
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
        // Components needed for Discovery command (check-first is TODO)
        vec![
            ComponentType::ServiceDiscovery, // Core service discovery
            ComponentType::UserInterface,    // User interface
            ComponentType::ConfigManager,    // Configuration management
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
        // Total width limit is 160
        const TOTAL_MAX_WIDTH: usize = 160;
        // Border and separator overhead
        const BORDER_OVERHEAD: usize = 7;

        // Calculate the maximum width of each column
        let name_width = services
            .iter()
            .map(|s| s.name.chars().count())
            .max()
            .unwrap_or(0)
            .max("Service Name".len());

        let tags_width = services
            .iter()
            .map(|s| s.tags.join(", ").chars().count())
            .max()
            .unwrap_or(0)
            .max("Tags".len());

        let desc_width = services
            .iter()
            .map(|s| {
                s.description
                    .as_deref()
                    .unwrap_or("No description")
                    .chars()
                    .count()
            })
            .max()
            .unwrap_or(0)
            .max("Description".len());

        let name_w = name_width;
        let tags_w = tags_width;
        let mut desc_w = desc_width;

        // If the total width is exceeded, truncate the Description
        if name_w + tags_w + desc_w + BORDER_OVERHEAD > TOTAL_MAX_WIDTH {
            let available = TOTAL_MAX_WIDTH - BORDER_OVERHEAD;
            let used = name_w + tags_w;
            desc_w = available.saturating_sub(used).max(10); // Description 至少 10 字符
        }

        // Generate table header
        let top_border = format!(
            "┌─{}─┬─{}─┬─{}─┐",
            "─".repeat(name_w),
            "─".repeat(tags_w),
            "─".repeat(desc_w)
        );
        let header = format!(
            "│ {:width$} │ {:tags_w$} │ {:desc_w$} │",
            "Service Name",
            "Tags",
            "Description",
            width = name_w,
            tags_w = tags_w,
            desc_w = desc_w
        );
        let separator = format!(
            "├─{}─┼─{}─┼─{}─┤",
            "─".repeat(name_w),
            "─".repeat(tags_w),
            "─".repeat(desc_w)
        );
        let bottom_border = format!(
            "└─{}─┴─{}─┴─{}─┘",
            "─".repeat(name_w),
            "─".repeat(tags_w),
            "─".repeat(desc_w)
        );

        println!("{top_border}");
        println!("{header}");
        println!("{separator}");

        for service in services {
            let tags_str = service.tags.join(", ");
            let description = service
                .description
                .as_deref()
                .unwrap_or("No description")
                .chars()
                .take(desc_w)
                .collect::<String>();

            println!(
                "│ {:name_w$} │ {:tags_w$} │ {:desc_w$} │",
                service.name,
                tags_str.chars().take(tags_w).collect::<String>(),
                description,
                name_w = name_w,
                tags_w = tags_w,
                desc_w = desc_w
            );
        }

        println!("{bottom_border}");
        println!();
    }

    /// Display service info
    fn display_service_info(&self, service: &ServiceInfo) {
        println!("📋 Selected service: {}", service.name);
        if let Some(desc) = &service.description {
            println!("📝 Description: {desc}");
        }
        // println!("🔗 URI: {}", service.uri);
        println!("🔐 Fingerprint: {}", service.fingerprint);
        let time = service
            .published_at
            .and_then(|published_at| chrono::DateTime::from_timestamp(published_at, 0))
            .map(|dt| {
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| "Unknown".to_string());
        println!("📅 Publication Time: {}", time);
        println!(
            "🏷️  Tags: {}",
            if service.tags.is_empty() {
                "(none)".to_string()
            } else {
                service.tags.join(", ")
            }
        );
        println!("📊 Methods count: {}", service.methods.len());
        println!();
    }

    #[allow(unused)]
    /// Display service details
    fn display_service_details(&self, details: &ServiceDetails) {
        println!("📖 {} Detailed Information:", details.info.name);
        println!("════════════════════════════════════════");

        println!("🏷️  Service Name: {}", details.info.name);
        // println!("🔗 URI: {}", details.info.uri);
        println!("🔐 Fingerprint: {}", details.info.fingerprint);

        if let Some(published_at) = details.info.published_at {
            let dt = chrono::DateTime::from_timestamp(published_at, 0)
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                })
                .unwrap_or_else(|| "Unknown".to_string());
            println!("📅 Publication Time: {}", dt);
        }

        if let Some(desc) = &details.info.description {
            println!("📝 Description: {desc}");
        }

        println!();
        println!("📋 Available Methods:");
        if details.info.methods.is_empty() {
            println!("  (None)");
        } else {
            for method in &details.info.methods {
                println!(
                    "  • {}: {} → {}",
                    method.name, method.input_type, method.output_type
                );
            }
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
        if details.proto_files.is_empty() {
            println!("  (None)");
        } else {
            for proto in &details.proto_files {
                println!("  • {} ({} services)", proto.name, proto.services.len());
            }
        }

        println!();
    }

    /// Export proto files
    async fn export_proto_files(
        &self,
        service: &ServiceInfo,
        service_discovery: &std::sync::Arc<dyn crate::core::ServiceDiscovery>,
    ) -> Result<()> {
        println!("📤 Exporting proto files for {}...", service.name);

        let proto_files = service_discovery.get_service_proto(&service.name).await?;

        let output_dir = std::path::Path::new("proto").join("remote");
        std::fs::create_dir_all(&output_dir)?;

        for proto in &proto_files {
            let file_path = output_dir.join(&proto.name);
            std::fs::write(&file_path, &proto.content)?;
            println!("✅ Exported: {}", file_path.display());
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
            alias: service.name.clone(),
            name: service.name.clone(),
            // uri: service.uri.clone(),
            fingerprint: Some(service.fingerprint.clone()),
        };

        // Check if a dependency with the same name already exists
        let config = config_manager
            .load_config(
                config_manager
                    .get_project_root()
                    .join("Actr.toml")
                    .as_path(),
            )
            .await?;

        // Check for duplicate dependency by name
        let existing_dep = config
            .dependencies
            .iter()
            .find(|dep| dep.name == service.name);

        let mut backup = None;
        if let Some(existing) = existing_dep {
            println!(
                "ℹ️  Dependency with name '{}' already exists (alias: '{}')",
                service.name, existing.alias
            );
            println!("   Skipping configuration update");
            // Still proceed with installation if user wants
        } else {
            println!("📝 Adding {} to configuration file...", service.name);

            // Backup configuration
            backup = Some(config_manager.backup_config().await?);

            // Update configuration
            match config_manager.update_dependency(&dependency_spec).await {
                Ok(_) => {
                    println!("✅ Added {} to configuration file", service.name);
                }
                Err(e) => {
                    if let Some(ref backup_val) = backup {
                        config_manager.restore_backup(backup_val.clone()).await?;
                    }
                    return Err(ActrCliError::Config {
                        message: format!("Configuration update failed: {e}"),
                    }
                    .into());
                }
            }
        }

        // TODO: Implement check-first validation pipeline.
        println!();
        println!("🔍 Verifying new dependency...");
        println!("ℹ️ check-first validation is not implemented yet; skipping.");
        if let Some(backup_val) = backup {
            config_manager.remove_backup(backup_val).await?;
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
                match container.get_install_pipeline() {
                    Ok(pipeline) => pipeline,
                    Err(_) => {
                        println!("ℹ️ Install pipeline is not implemented yet; skipping.");
                        return Ok(CommandResult::Success(
                            "Dependency added; install pending".to_string(),
                        ));
                    }
                }
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

        // Discovery command requires minimal components while check-first is TODO.
        assert!(components.contains(&ComponentType::ServiceDiscovery));
        assert!(components.contains(&ComponentType::UserInterface));
        assert!(components.contains(&ComponentType::ConfigManager));
        assert!(!components.contains(&ComponentType::DependencyResolver));
        assert!(!components.contains(&ComponentType::NetworkValidator));
        assert!(!components.contains(&ComponentType::FingerprintValidator));
        assert!(!components.contains(&ComponentType::CacheManager));
        assert!(!components.contains(&ComponentType::ProtoProcessor));
    }
}
