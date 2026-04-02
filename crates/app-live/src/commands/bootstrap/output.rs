use std::path::Path;

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

pub fn print_smoke_ready_summary(config_path: &Path) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Smoke bootstrap config written");
    println!("Config: {}", config_path.display());
    println!("Next: app-live targets candidates --config {quoted_config_path}");
    println!(
        "Next: app-live targets adopt --config {quoted_config_path} --adoptable-revision ADOPTABLE_REVISION",
    );
}

pub fn print_smoke_target_anchor_summary(config_path: &Path) {
    let quoted_config_path = shell_quote(config_path.display().to_string());
    println!("Smoke bootstrap target anchor written");
    println!("Config: {}", config_path.display());
    println!("Next: app-live bootstrap --config {quoted_config_path}");
    println!("Next: app-live targets show-current --config {quoted_config_path}");
}

fn shell_quote(value: String) -> String {
    let escaped = value.replace('\'', r"'\''");
    format!("'{escaped}'")
}
