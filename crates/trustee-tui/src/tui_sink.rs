//! TuiSink - Bridges ABK OutputEvents to TUI message channel
//!
//! Task 53: Implements the `OutputSink` trait so that structured events
//! emitted by the orchestration layer are forwarded directly to the
//! ratatui display via `mpsc::UnboundedSender<TuiMessage>`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

use abk::orchestration::output::{OutputEvent, OutputSink, SharedSink};

use crate::app::TuiMessage;

/// A sink that forwards ABK `OutputEvent`s to the TUI event channel.
///
/// Each event variant is mapped to an appropriate `TuiMessage` so the
/// ratatui render loop can display it in the output pane.
pub struct TuiSink {
    tx: mpsc::UnboundedSender<TuiMessage>,
    /// Whether we're inside a reasoning block (to start it on a new line).
    in_reasoning: AtomicBool,
}

impl TuiSink {
    /// Create a new `TuiSink` wrapping the given channel sender.
    pub fn new(tx: mpsc::UnboundedSender<TuiMessage>) -> Self {
        Self { tx, in_reasoning: AtomicBool::new(false) }
    }

    /// Convenience helper: wrap in an `Arc` for use as `SharedSink`.
    pub fn shared(tx: mpsc::UnboundedSender<TuiMessage>) -> SharedSink {
        Arc::new(Self::new(tx))
    }
}

impl OutputSink for TuiSink {
    fn emit(&self, event: OutputEvent) {
        let msg = match event {
            // Streaming chunks — append to last line (print-style) instead of
            // creating a new line for each chunk (println-style).
            // This makes LLM thinking/response text flow naturally within a line.
            OutputEvent::StreamingChunk { delta } => {
                if delta.is_empty() {
                    return;
                }
                // If transitioning from reasoning to content, start a new line.
                if self.in_reasoning.swap(false, Ordering::Relaxed) {
                    let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                }
                // Use a dedicated message type so handle_workflow_message can append
                // rather than push a new line.
                TuiMessage::StreamDelta(delta)
            }

            // Full LLM responses — display with model info
            OutputEvent::LlmResponse { text, model } => {
                TuiMessage::OutputLine(format!("[{}] {}", model, text))
            }

            // General informational messages
            OutputEvent::Info { message } => TuiMessage::OutputLine(message),

            // Workflow lifecycle events
            OutputEvent::WorkflowStarted { task_description } => {
                TuiMessage::OutputLine(format!("🚀 Workflow started: {}", task_description))
            }

            OutputEvent::WorkflowCompleted { reason, iterations } => {
                TuiMessage::OutputLine(format!(
                    "✅ Workflow completed after {} iterations: {}",
                    iterations, reason
                ))
            }

            OutputEvent::IterationStarted { iteration, context_tokens } => {
                TuiMessage::OutputLine(format!(
                    "📡 Iteration {} | Context = {} tokens",
                    iteration, context_tokens
                ))
            }

            // API call events
            OutputEvent::ApiCallStarted {
                call_number,
                model,
                tool_count,
                streaming,
            } => {
                let mode = if streaming { "Streaming" } else { "Non-streaming" };
                TuiMessage::OutputLine(format!(
                    "🔥 API Call {} | {} | Model: {} | Tools: {}",
                    call_number, mode, model, tool_count
                ))
            }

            // Tool execution events
            OutputEvent::ToolsExecuting { tool_names } => {
                TuiMessage::OutputLine(format!(
                    "🔧 Executing {} tools: [{}]",
                    tool_names.len(),
                    tool_names.join(", ")
                ))
            }

            OutputEvent::ToolCompleted {
                tool_name,
                success,
                content,
            } => {
                let status = if success { "Result" } else { "Error" };
                TuiMessage::OutputLine(format!("Tool: {}\n{}: {}", tool_name, status, content))
            }

            // Error events
            OutputEvent::Error { message, context } => {
                if let Some(ctx) = context {
                    TuiMessage::OutputLine(format!("❌ Error: {} — {}", message, ctx))
                } else {
                    TuiMessage::OutputLine(format!("❌ Error: {}", message))
                }
            }

            // Reasoning chunks — send as ReasoningDelta for grey rendering
            // (ratatui applies DarkGray style via the \x01 marker convention)
            OutputEvent::ReasoningChunk { delta } => {
                if delta.is_empty() {
                    return;
                }
                // First reasoning chunk → push a new line so it doesn't append
                // to the previous OutputLine (e.g. API Call info).
                if !self.in_reasoning.swap(true, Ordering::Relaxed) {
                    let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                }
                TuiMessage::ReasoningDelta(delta)
            }
        };

        // Best-effort send — if the receiver is dropped the TUI has exited
        let _ = self.tx.send(msg);
    }
}
