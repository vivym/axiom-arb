use std::collections::BTreeMap;

use domain::{ActivationDecision, ExecutionMode};

use crate::rollout::{index_rules, resolve_rule, RolloutRule, RolloutRuleMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationPolicy {
    capability_matrix: RolloutRuleMap,
    overlays: BTreeMap<(String, String), ExecutionMode>,
    policy_version: String,
    clamp_negrisk_live: bool,
    clamp_negrisk_to_shadow: bool,
}

impl ActivationPolicy {
    pub fn phase_one_defaults() -> Self {
        Self {
            capability_matrix: index_rules(vec![
                RolloutRule::new(
                    "full-set",
                    "default",
                    ExecutionMode::Live,
                    "phase-one-fullset-default",
                ),
                RolloutRule::new(
                    "neg-risk",
                    "default",
                    ExecutionMode::Shadow,
                    "phase-one-negrisk-default",
                ),
            ]),
            overlays: BTreeMap::new(),
            policy_version: "phase-one-defaults".to_owned(),
            clamp_negrisk_live: true,
            clamp_negrisk_to_shadow: false,
        }
    }

    pub fn from_rules(policy_version: impl Into<String>, rules: Vec<RolloutRule>) -> Self {
        Self {
            capability_matrix: index_rules(rules),
            overlays: BTreeMap::new(),
            policy_version: policy_version.into(),
            clamp_negrisk_live: false,
            clamp_negrisk_to_shadow: false,
        }
    }

    pub fn with_real_user_shadow_smoke(mut self) -> Self {
        self.clamp_negrisk_to_shadow = true;
        self
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
        self.activation_for(route, scope, "").mode
    }

    pub fn activation_for(
        &self,
        route: &str,
        scope: &str,
        snapshot_id: &str,
    ) -> ActivationDecision {
        let (mode, reason, matched_rule_id) = if let Some(mode) = self
            .overlays
            .get(&(route.to_owned(), scope.to_owned()))
            .copied()
        {
            (
                mode,
                format!("activation-overlay snapshot={snapshot_id}"),
                None,
            )
        } else if let Some(rule) = resolve_rule(&self.capability_matrix, route, scope) {
            (
                rule.mode,
                format!("activation-rollout snapshot={snapshot_id}"),
                Some(rule.rule_id.clone()),
            )
        } else {
            (
                ExecutionMode::Disabled,
                format!("activation-missing snapshot={snapshot_id}"),
                None,
            )
        };

        ActivationDecision::new(
            self.clamp_mode(route, mode),
            scope,
            reason,
            self.policy_version.clone(),
            matched_rule_id,
        )
    }

    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }

    fn clamp_mode(&self, route: &str, mode: ExecutionMode) -> ExecutionMode {
        if self.clamp_negrisk_to_shadow {
            return clamp_real_user_shadow_smoke_mode(route, mode);
        }

        if self.clamp_negrisk_live {
            return clamp_phase_one_mode(route, mode);
        }

        mode
    }
}

fn clamp_phase_one_mode(route: &str, mode: ExecutionMode) -> ExecutionMode {
    if route == crate::negrisk::ROUTE {
        return crate::negrisk::phase_one_effective_mode(mode);
    }

    mode
}

fn clamp_real_user_shadow_smoke_mode(route: &str, mode: ExecutionMode) -> ExecutionMode {
    if route == crate::negrisk::ROUTE && mode == ExecutionMode::Live {
        return ExecutionMode::Shadow;
    }

    mode
}
