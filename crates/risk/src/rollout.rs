use std::collections::BTreeMap;

use domain::ExecutionMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutRule {
    pub route: String,
    pub scope: String,
    pub mode: ExecutionMode,
    pub rule_id: String,
}

impl RolloutRule {
    pub fn new(
        route: impl Into<String>,
        scope: impl Into<String>,
        mode: ExecutionMode,
        rule_id: impl Into<String>,
    ) -> Self {
        Self {
            route: route.into(),
            scope: scope.into(),
            mode,
            rule_id: rule_id.into(),
        }
    }
}

pub type RolloutRuleMap = BTreeMap<(String, String), RolloutRule>;

pub fn index_rules(rules: Vec<RolloutRule>) -> RolloutRuleMap {
    rules
        .into_iter()
        .map(|rule| ((rule.route.clone(), rule.scope.clone()), rule))
        .collect()
}

pub fn resolve_rule<'a>(
    rules: &'a RolloutRuleMap,
    route: &str,
    scope: &str,
) -> Option<&'a RolloutRule> {
    rules
        .get(&(route.to_owned(), scope.to_owned()))
        .or_else(|| rules.get(&(route.to_owned(), "default".to_owned())))
}
