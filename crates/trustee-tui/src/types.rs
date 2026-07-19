//! Type definitions for the TUI application.
//!
//! Re-exports shared types from trustee-core.

// Re-export types that have been moved to trustee-core
pub use trustee_core::types::{
    FocusPanel, McpServerStatus, TuiMessage, WorkflowState,
};
