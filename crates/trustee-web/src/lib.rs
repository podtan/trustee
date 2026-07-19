//! Trustee Web — static frontend embedded via rust-embed.
//!
//! The HTML/CSS/JS files live in `static/` and are compiled into the binary.
//! trustee-api serves them via `Asset::get(path)`.

use rust_embed::Embed;

#[derive(Embed)]
#[folder = "static/"]
pub struct Asset;
