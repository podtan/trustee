//! Trustee Core — shared types, session state, and workflow logic.
//!
//! This crate is the shared foundation used by both `trustee-tui` (terminal UI)
//! and the upcoming `trustee-api` (HTTP/WebSocket server). It contains:
//!
//! - **Types**: enums and structs that model the agent's state machine
//! - **Session**: the core `Session` struct (agent state without UI concerns)
//! - **Workflow**: command execution, handoff, and message handling
//! - **Config**: auto-handoff and color parsing from TOML

pub mod config;
pub mod session;
pub mod sessions;
pub mod types;
