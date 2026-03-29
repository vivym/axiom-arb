use std::{
    error::Error,
    fmt::{self, Write as _},
    fs,
};

use crate::cli::{InitArgs, InitMode};

#[derive(Debug)]
pub struct InitError {
    message: String,
}

impl InitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for InitError {}

impl From<std::io::Error> for InitError {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

pub fn execute(args: InitArgs) -> Result<(), Box<dyn Error>> {
    let config = render_config(&args);
    if let Some(parent) = args.config.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.config, config)?;
    Ok(())
}

fn render_config(args: &InitArgs) -> String {
    let mode = match args.mode {
        InitMode::Paper => "paper",
        InitMode::Live => "live",
    };

    match args.mode {
        InitMode::Paper => render_paper_config(mode),
        InitMode::Live => render_live_config(mode, args.real_user_shadow_smoke),
    }
}

fn render_paper_config(mode: &str) -> String {
    let mut output = String::new();
    writeln!(&mut output, "[runtime]").unwrap();
    writeln!(&mut output, "mode = \"{mode}\"").unwrap();
    output
}

fn render_live_config(mode: &str, real_user_shadow_smoke: bool) -> String {
    let mut output = String::new();
    writeln!(&mut output, "[runtime]").unwrap();
    writeln!(&mut output, "mode = \"{mode}\"").unwrap();
    writeln!(
        &mut output,
        "real_user_shadow_smoke = {}",
        if real_user_shadow_smoke {
            "true"
        } else {
            "false"
        }
    )
    .unwrap();
    writeln!(&mut output).unwrap();
    writeln!(&mut output, "[polymarket.source]").unwrap();
    writeln!(&mut output, "clob_host = \"https://clob.polymarket.com\"").unwrap();
    writeln!(
        &mut output,
        "data_api_host = \"https://data-api.polymarket.com\""
    )
    .unwrap();
    writeln!(
        &mut output,
        "relayer_host = \"https://relayer-v2.polymarket.com\""
    )
    .unwrap();
    writeln!(
        &mut output,
        "market_ws_url = \"wss://ws-subscriptions-clob.polymarket.com/ws/market\""
    )
    .unwrap();
    writeln!(
        &mut output,
        "user_ws_url = \"wss://ws-subscriptions-clob.polymarket.com/ws/user\""
    )
    .unwrap();
    writeln!(&mut output, "heartbeat_interval_seconds = 15").unwrap();
    writeln!(&mut output, "relayer_poll_interval_seconds = 5").unwrap();
    writeln!(&mut output, "metadata_refresh_interval_seconds = 60").unwrap();
    writeln!(&mut output).unwrap();
    writeln!(&mut output, "[polymarket.account]").unwrap();
    writeln!(&mut output, "address = \"0xYOUR_ADDRESS\"").unwrap();
    writeln!(&mut output, "funder_address = \"0xYOUR_FUNDER_ADDRESS\"").unwrap();
    writeln!(&mut output, "signature_type = \"eoa\"").unwrap();
    writeln!(&mut output, "wallet_route = \"eoa\"").unwrap();
    writeln!(&mut output, "api_key = \"YOUR_API_KEY\"").unwrap();
    writeln!(&mut output, "secret = \"YOUR_API_SECRET\"").unwrap();
    writeln!(&mut output, "passphrase = \"YOUR_PASSPHRASE\"").unwrap();
    writeln!(&mut output).unwrap();
    writeln!(&mut output, "[polymarket.relayer_auth]").unwrap();
    writeln!(&mut output, "kind = \"builder_api_key\"").unwrap();
    writeln!(&mut output, "api_key = \"YOUR_BUILDER_API_KEY\"").unwrap();
    writeln!(&mut output, "secret = \"YOUR_BUILDER_SECRET\"").unwrap();
    writeln!(&mut output, "passphrase = \"YOUR_BUILDER_PASSPHRASE\"").unwrap();
    writeln!(&mut output).unwrap();
    writeln!(&mut output, "[negrisk.target_source]").unwrap();
    writeln!(&mut output, "source = \"adopted\"").unwrap();
    writeln!(
        &mut output,
        "operator_target_revision = \"YOUR_OPERATOR_TARGET_REVISION\""
    )
    .unwrap();
    writeln!(&mut output).unwrap();
    writeln!(&mut output, "[negrisk.rollout]").unwrap();
    writeln!(&mut output, "approved_families = [\"YOUR_FAMILY_ID\"]").unwrap();
    writeln!(&mut output, "ready_families = [\"YOUR_FAMILY_ID\"]").unwrap();
    output
}
