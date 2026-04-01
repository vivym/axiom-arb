use std::path::Path;

pub fn print_ready_summary(config_path: &Path) {
    println!("Paper bootstrap ready");
    println!("Config: {}", config_path.display());
    println!(
        "Runtime not started. Re-run with --start or run app-live run --config {}",
        config_path.display()
    );
}

pub fn print_starting_runtime(config_path: &Path) {
    println!(
        "Paper bootstrap ready. Starting runtime with {}",
        config_path.display()
    );
}
