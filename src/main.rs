//! Trustee - A general-purpose agent that can morph into different specialized agents

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    abk::cli::run_configured_cli_from_config("config/trustee.toml").await
}