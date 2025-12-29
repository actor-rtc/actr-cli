use anyhow::{Result, anyhow};
use async_trait::async_trait;
use heck::ToUpperCamelCase;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::SystemTime;

use crate::core::{
    AvailabilityStatus, HealthStatus, MethodDefinition, ProtoFile, ServiceDefinition,
    ServiceDetails, ServiceDiscovery, ServiceFilter, ServiceInfo,
};

#[derive(Clone)]
struct CatalogEntry {
    info: ServiceInfo,
    tags: Vec<String>,
    dependencies: Vec<String>,
    proto_files: Vec<ProtoFile>,
}

pub struct NetworkServiceDiscovery {
    catalog: Vec<CatalogEntry>,
}

impl NetworkServiceDiscovery {
    pub fn new() -> Self {
        let catalog = vec![
            Self::build_entry(
                "user-service",
                "1.0.0",
                "User profile and authentication service",
                &["user", "auth"],
                &[],
            ),
            Self::build_entry(
                "order-service",
                "1.2.0",
                "Order management service",
                &["order", "workflow"],
                &["actr://user-service/"],
            ),
            Self::build_entry(
                "payment-service",
                "2.0.0",
                "Payments and billing service",
                &["payment", "billing"],
                &["actr://order-service/"],
            ),
        ];

        Self { catalog }
    }

    fn build_entry(
        name: &str,
        version: &str,
        description: &str,
        tags: &[&str],
        dependencies: &[&str],
    ) -> CatalogEntry {
        let methods = Self::build_methods(name);
        let info = ServiceInfo {
            name: name.to_string(),
            uri: format!("actr://{name}/"),
            version: version.to_string(),
            fingerprint: Self::fingerprint_for(name),
            description: Some(description.to_string()),
            methods: methods.clone(),
        };
        let proto_files = vec![Self::build_proto_file(name, &methods)];

        CatalogEntry {
            info,
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
            dependencies: dependencies.iter().map(|dep| (*dep).to_string()).collect(),
            proto_files,
        }
    }

    fn build_methods(name: &str) -> Vec<MethodDefinition> {
        let service_name = name.to_upper_camel_case();
        vec![
            MethodDefinition {
                name: format!("Get{service_name}"),
                input_type: format!("Get{service_name}Request"),
                output_type: format!("Get{service_name}Response"),
            },
            MethodDefinition {
                name: format!("List{service_name}"),
                input_type: format!("List{service_name}Request"),
                output_type: format!("List{service_name}Response"),
            },
        ]
    }

    fn build_proto_file(name: &str, methods: &[MethodDefinition]) -> ProtoFile {
        let service_name = format!("{}Service", name.to_upper_camel_case());
        let package_name = name.replace('-', "_");
        let mut service_methods = String::new();
        let mut message_defs = String::new();

        for method in methods {
            service_methods.push_str(&format!(
                "  rpc {} ({}) returns ({});\n",
                method.name, method.input_type, method.output_type
            ));
            message_defs.push_str(&format!("message {} {{}}\n", method.input_type));
            message_defs.push_str(&format!("message {} {{}}\n", method.output_type));
        }

        let content = format!(
            "syntax = \"proto3\";\n\npackage {package_name};\n\nservice {service_name} {{\n{service_methods}}}\n\n{message_defs}",
        );

        ProtoFile {
            name: format!("{name}.proto"),
            path: PathBuf::from(format!("proto/{name}.proto")),
            content,
            services: vec![ServiceDefinition {
                name: service_name,
                methods: methods.to_vec(),
            }],
        }
    }

    fn fingerprint_for(name: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(name.as_bytes());
        let digest = hasher.finalize();
        let hex = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        format!("sha256:{hex}")
    }

    fn parse_actr_uri(&self, uri: &str) -> Result<String> {
        if !uri.starts_with("actr://") {
            return Err(anyhow!("Invalid actr:// URI: {uri}"));
        }

        let without_scheme = &uri["actr://".len()..];
        let name_end = without_scheme
            .find(|c| ['/', '?'].contains(&c))
            .unwrap_or(without_scheme.len());
        let name = without_scheme[..name_end].trim();
        if name.is_empty() {
            return Err(anyhow!("Invalid actr:// URI: {uri}"));
        }

        Ok(name.to_string())
    }

    fn matches_filter(entry: &CatalogEntry, filter: &ServiceFilter) -> bool {
        if let Some(pattern) = &filter.name_pattern
            && !Self::matches_pattern(&entry.info.name, pattern)
        {
            return false;
        }

        if let Some(version_range) = &filter.version_range
            && entry.info.version != *version_range
        {
            return false;
        }

        if let Some(tags) = &filter.tags {
            let has_all = tags.iter().all(|tag| entry.tags.iter().any(|t| t == tag));
            if !has_all {
                return false;
            }
        }

        true
    }

    fn matches_pattern(value: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        let segments: Vec<&str> = pattern.split('*').collect();
        if segments.len() == 1 {
            return value == pattern;
        }

        if !pattern.starts_with('*')
            && let Some(first) = segments.first()
            && !value.starts_with(first)
        {
            return false;
        }

        if !pattern.ends_with('*')
            && let Some(last) = segments.last()
            && !value.ends_with(last)
        {
            return false;
        }

        let mut search_start = 0;
        let end_limit = if !pattern.ends_with('*') {
            value
                .len()
                .saturating_sub(segments.last().unwrap_or(&"").len())
        } else {
            value.len()
        };

        for (index, segment) in segments.iter().enumerate() {
            if segment.is_empty() {
                continue;
            }
            if index == 0 && !pattern.starts_with('*') {
                search_start = segment.len();
                continue;
            }
            if index == segments.len() - 1 && !pattern.ends_with('*') {
                continue;
            }
            if let Some(found) = value[search_start..end_limit].find(segment) {
                search_start += found + segment.len();
            } else {
                return false;
            }
        }

        true
    }

    fn find_entry(&self, name: &str) -> Option<&CatalogEntry> {
        self.catalog.iter().find(|entry| entry.info.name == name)
    }
}

impl Default for NetworkServiceDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceDiscovery for NetworkServiceDiscovery {
    async fn discover_services(&self, filter: Option<&ServiceFilter>) -> Result<Vec<ServiceInfo>> {
        let services = match filter {
            Some(filter) => self
                .catalog
                .iter()
                .filter(|entry| Self::matches_filter(entry, filter))
                .map(|entry| entry.info.clone())
                .collect(),
            None => self
                .catalog
                .iter()
                .map(|entry| entry.info.clone())
                .collect(),
        };
        Ok(services)
    }

    async fn get_service_details(&self, uri: &str) -> Result<ServiceDetails> {
        let name = self.parse_actr_uri(uri)?;
        let entry = self
            .find_entry(&name)
            .ok_or_else(|| anyhow!("Service not found: {name}"))?;

        Ok(ServiceDetails {
            info: entry.info.clone(),
            proto_files: entry.proto_files.clone(),
            dependencies: entry.dependencies.clone(),
        })
    }

    async fn check_service_availability(&self, uri: &str) -> Result<AvailabilityStatus> {
        let name = self.parse_actr_uri(uri)?;
        let available = self.find_entry(&name).is_some();

        Ok(AvailabilityStatus {
            is_available: available,
            last_seen: available.then(SystemTime::now),
            health: if available {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unknown
            },
        })
    }

    async fn get_service_proto(&self, uri: &str) -> Result<Vec<ProtoFile>> {
        let name = self.parse_actr_uri(uri)?;
        let entry = self
            .find_entry(&name)
            .ok_or_else(|| anyhow!("Service not found: {name}"))?;
        Ok(entry.proto_files.clone())
    }
}
