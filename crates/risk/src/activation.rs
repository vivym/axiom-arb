use std::collections::BTreeMap;

use domain::ExecutionMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationPolicy {
    capability_matrix: BTreeMap<(String, String), ExecutionMode>,
    overlays: BTreeMap<(String, String), ExecutionMode>,
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

    pub fn with_overlay(
        mut self,
        route: impl Into<String>,
        scope: impl Into<String>,
        mode: ExecutionMode,
    ) -> Self {
        self.overlays.insert((route.into(), scope.into()), mode);
        self
    }

    pub fn mode_for_route(&self, route: &str, scope: &str) -> ExecutionMode {
        clamp_phase_one_mode(
            route,
            self.overlays
                .get(&(route.to_owned(), scope.to_owned()))
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
                .unwrap_or(ExecutionMode::Disabled),
        )
    }

    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }
}

fn clamp_phase_one_mode(route: &str, mode: ExecutionMode) -> ExecutionMode {
    if route == crate::negrisk::ROUTE {
        return crate::negrisk::clamp_mode(mode);
    }

    mode
}
