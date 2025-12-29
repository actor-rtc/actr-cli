use anyhow::Result;
use async_trait::async_trait;

use super::{
    ConflictReport, ConflictType, DependencyGraph, DependencyResolver, DependencySpec,
    ResolvedDependency,
};

pub struct DefaultDependencyResolver;

impl DefaultDependencyResolver {
    pub fn new() -> Self {
        Self
    }

    fn parse_actr_uri(&self, spec: &str) -> Result<DependencySpec> {
        let without_scheme = spec
            .strip_prefix("actr://")
            .ok_or_else(|| anyhow::anyhow!("Invalid actr:// URI: {spec}"))?;
        let name_end = without_scheme
            .find(|c| ['/', '?'].contains(&c))
            .unwrap_or(without_scheme.len());
        let name = without_scheme[..name_end].trim();
        if name.is_empty() {
            return Err(anyhow::anyhow!("Invalid actr:// URI: {spec}"));
        }

        let mut version = None;
        let mut fingerprint = None;
        if let Some(query_start) = spec.find('?') {
            let query = &spec[query_start + 1..];
            for pair in query.split('&') {
                if pair.is_empty() {
                    continue;
                }
                let mut iter = pair.splitn(2, '=');
                let key = iter.next().unwrap_or_default();
                let value = iter.next().unwrap_or_default();
                match key {
                    "version" if !value.is_empty() => {
                        version = Some(value.to_string());
                    }
                    "fingerprint" if !value.is_empty() => {
                        fingerprint = Some(value.to_string());
                    }
                    _ => {}
                }
            }
        }

        Ok(DependencySpec {
            name: name.to_string(),
            uri: spec.to_string(),
            version,
            fingerprint,
        })
    }

    fn parse_versioned_spec(&self, spec: &str) -> Result<DependencySpec> {
        let (name, version) = spec
            .rsplit_once('@')
            .ok_or_else(|| anyhow::anyhow!("Invalid package specification: {spec}"))?;
        if name.is_empty() || version.is_empty() {
            return Err(anyhow::anyhow!("Invalid package specification: {spec}"));
        }

        let uri = format!("actr://{name}/?version={version}");
        Ok(DependencySpec {
            name: name.to_string(),
            uri,
            version: Some(version.to_string()),
            fingerprint: None,
        })
    }

    fn parse_simple_spec(&self, spec: &str) -> Result<DependencySpec> {
        let name = spec.trim();
        if name.is_empty() {
            return Err(anyhow::anyhow!("Invalid package specification: {spec}"));
        }
        let uri = format!("actr://{name}/");
        Ok(DependencySpec {
            name: name.to_string(),
            uri,
            version: None,
            fingerprint: None,
        })
    }
}

impl Default for DefaultDependencyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DependencyResolver for DefaultDependencyResolver {
    async fn resolve_spec(&self, spec: &str) -> Result<DependencySpec> {
        if spec.starts_with("actr://") {
            return self.parse_actr_uri(spec);
        }

        if spec.contains('@') {
            return self.parse_versioned_spec(spec);
        }

        self.parse_simple_spec(spec)
    }

    async fn resolve_dependencies(
        &self,
        specs: &[DependencySpec],
    ) -> Result<Vec<ResolvedDependency>> {
        let mut resolved = Vec::with_capacity(specs.len());

        for spec in specs {
            resolved.push(ResolvedDependency {
                spec: spec.clone(),
                uri: spec.uri.clone(),
                resolved_version: spec.version.clone().unwrap_or_else(|| "latest".to_string()),
                fingerprint: spec.fingerprint.clone().unwrap_or_default(),
                proto_files: Vec::new(),
            });
        }

        Ok(resolved)
    }

    async fn check_conflicts(&self, deps: &[ResolvedDependency]) -> Result<Vec<ConflictReport>> {
        let mut conflicts = Vec::new();

        for i in 0..deps.len() {
            for j in (i + 1)..deps.len() {
                if deps[i].spec.name != deps[j].spec.name {
                    continue;
                }

                if deps[i].resolved_version != deps[j].resolved_version {
                    conflicts.push(ConflictReport {
                        dependency_a: deps[i].spec.name.clone(),
                        dependency_b: deps[j].spec.name.clone(),
                        conflict_type: ConflictType::VersionConflict,
                        description: format!(
                            "Dependency {} has conflicting versions: {} vs {}",
                            deps[i].spec.name, deps[i].resolved_version, deps[j].resolved_version
                        ),
                    });
                }

                if !deps[i].fingerprint.is_empty()
                    && !deps[j].fingerprint.is_empty()
                    && deps[i].fingerprint != deps[j].fingerprint
                {
                    conflicts.push(ConflictReport {
                        dependency_a: deps[i].spec.name.clone(),
                        dependency_b: deps[j].spec.name.clone(),
                        conflict_type: ConflictType::FingerprintMismatch,
                        description: format!(
                            "Dependency {} has conflicting fingerprints",
                            deps[i].spec.name
                        ),
                    });
                }
            }
        }

        Ok(conflicts)
    }

    async fn build_dependency_graph(&self, deps: &[ResolvedDependency]) -> Result<DependencyGraph> {
        let mut nodes = Vec::new();
        for dep in deps {
            if !nodes.contains(&dep.spec.name) {
                nodes.push(dep.spec.name.clone());
            }
        }

        Ok(DependencyGraph {
            nodes,
            edges: Vec::new(),
            has_cycles: false,
        })
    }
}
