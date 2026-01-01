//! Trustee - A general-purpose agent that can morph into different specialized agents

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Simple built-in helper: if the user runs the binary with `hello`/`hi` or `say hello`/`say hi`,
    // print the corresponding greeting and exit. Otherwise, run the configured ABK CLI as before.
    let mut args = std::env::args().skip(1);
    abk::cli::run_configured_cli_from_config("config/trustee.toml").await
}
