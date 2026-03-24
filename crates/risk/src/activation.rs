use std::collections::BTreeMap;

use domain::ExecutionMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationPolicy {
    capability_matrix: BTreeMap<(String, String), ExecutionMode>,
    overlays: BTreeMap<String, ExecutionMode>,
    policy_version: String,
}

impl ActivationPolicy {
    pub fn phase_one_defaults() -> Self {
        let mut capability_matrix = BTreeMap::new();
        capability_matrix.insert(
            ("full-set".to_owned(), "default".to_owned()),
            ExecutionMode::Live,
        );
        capability_matrix.insert(
            ("neg-risk".to_owned(), "default".to_owned()),
            ExecutionMode::Shadow,
        );

        Self {
            capability_matrix,
            overlays: BTreeMap::new(),
            policy_version: "phase-one-defaults".to_owned(),
        }
    }

    pub fn with_overlay(mut self, scope: impl Into<String>, mode: ExecutionMode) -> Self {
        self.overlays.insert(scope.into(), mode);
        self
    }

    pub fn mode_for_route(&self, route: &str, scope: &str) -> ExecutionMode {
        self.overlays
            .get(scope)
            .copied()
            .or_else(|| {
                self.capability_matrix
                    .get(&(route.to_owned(), scope.to_owned()))
                    .copied()
            })
            .or_else(|| {
                self.capability_matrix
                    .get(&(route.to_owned(), "default".to_owned()))
                    .copied()
            })
            .unwrap_or(ExecutionMode::Disabled)
    }

    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }
}
