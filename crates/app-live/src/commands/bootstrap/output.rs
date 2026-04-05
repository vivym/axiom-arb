use std::path::Path;

use super::flow::DiscoveryArtifactsSource;

pub fn print_ready_summary(config_path: &Path) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Paper bootstrap ready");
    println!("Config: {}", config_path.display());
    println!(
        "Runtime not started. Re-run with --start or use: app-live run --config {quoted_config_path}",
    );
}

pub fn print_starting_runtime(config_path: &Path) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Paper bootstrap ready. Starting runtime with config {quoted_config_path}",);
}

pub fn print_starting_smoke_runtime(config_path: &Path) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!(
        "Smoke bootstrap reached shadow-work-ready smoke startup. Starting runtime with config {quoted_config_path}",
    );
}

pub fn print_smoke_discovery_completed(
    source: DiscoveryArtifactsSource,
    adoptable_revisions: &[String],
    recommended_adoptable_revision: Option<&str>,
) {
    match source {
        DiscoveryArtifactsSource::FreshDiscover => println!("Discovery completed"),
        DiscoveryArtifactsSource::Persisted => println!("Using persisted discovery artifacts"),
    }
    println!("Adoptable revisions: {}", adoptable_revisions.join(", "));
    println!(
        "Recommended: {}",
        recommended_adoptable_revision.unwrap_or("none")
    );
}

pub fn print_waiting_for_explicit_adoption_confirmation(config_path: &Path) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Waiting for explicit adoption confirmation");
    println!(
        "Next: rerun app-live bootstrap --config {quoted_config_path} and enter one of the listed adoptable revisions"
    );
}

pub fn print_smoke_discovery_ready_not_adoptable(
    source: DiscoveryArtifactsSource,
    config_path: &Path,
    reasons: &[String],
) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    match source {
        DiscoveryArtifactsSource::FreshDiscover => {
            println!("Discovery completed but no adoptable revisions were produced")
        }
        DiscoveryArtifactsSource::Persisted => {
            println!("Using persisted discovery artifacts");
            println!("No adoptable revisions were produced");
        }
    }
    println!(
        "Reasons: {}",
        if reasons.is_empty() {
            "none recorded".to_owned()
        } else {
            reasons.join(", ")
        }
    );
    println!("Next: rerun app-live discover --config {quoted_config_path}");
}

pub fn print_smoke_preflight_only_summary(config_path: &Path, family_ids: &[String]) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Smoke bootstrap reached preflight-ready smoke startup");
    println!("Config: {}", config_path.display());
    println!("Adopted families: {}", family_ids.join(", "));
    println!("Next: app-live bootstrap --config {quoted_config_path}");
}

pub fn print_smoke_rollout_ready_summary(config_path: &Path, family_ids: &[String]) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Smoke bootstrap reached shadow-work-ready smoke startup");
    println!("Config: {}", config_path.display());
    println!("Rollout families: {}", family_ids.join(", "));
    println!("Next: app-live bootstrap --config {quoted_config_path}");
}

fn shell_quote(value: String) -> String {
    let escaped = value.replace('\'', r"'\''");
    format!("'{escaped}'")
}
