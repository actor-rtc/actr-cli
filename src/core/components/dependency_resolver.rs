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
}

impl Default for DefaultDependencyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DependencyResolver for DefaultDependencyResolver {
    async fn resolve_spec(&self, _spec: &str) -> Result<DependencySpec> {
        todo!()
    }

    async fn resolve_dependencies(
        &self,
        specs: &[DependencySpec],
    ) -> Result<Vec<ResolvedDependency>> {
        let mut resolved = Vec::with_capacity(specs.len());

        for spec in specs {
            resolved.push(ResolvedDependency {
                spec: spec.clone(),
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
                // Conflict if same alias is used (should not happen if from same Actr.toml, but could from merged configs)
                if deps[i].spec.alias == deps[j].spec.alias {
                    // Same alias is always a conflict if they point to different things
                    if deps[i].spec.name != deps[j].spec.name
                        || deps[i].fingerprint != deps[j].fingerprint
                    {
                        conflicts.push(ConflictReport {
                            dependency_a: deps[i].spec.alias.clone(),
                            dependency_b: deps[j].spec.alias.clone(),
                            conflict_type: ConflictType::VersionConflict,
                            description: format!(
                                "Dependency alias '{}' is duplicated with different targets",
                                deps[i].spec.alias
                            ),
                        });
                        continue;
                    }
                }

                // Conflict if same package name has different fingerprints
                if deps[i].spec.name != deps[j].spec.name {
                    continue;
                }

                if !deps[i].fingerprint.is_empty()
                    && !deps[j].fingerprint.is_empty()
                    && deps[i].fingerprint != deps[j].fingerprint
                {
                    conflicts.push(ConflictReport {
                        dependency_a: format!("{} ({})", deps[i].spec.name, deps[i].spec.alias),
                        dependency_b: format!("{} ({})", deps[j].spec.name, deps[j].spec.alias),
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
            if !nodes.contains(&dep.spec.alias) {
                nodes.push(dep.spec.alias.clone());
            }
        }

        Ok(DependencyGraph {
            nodes,
            edges: Vec::new(),
            has_cycles: false,
        })
    }
}
