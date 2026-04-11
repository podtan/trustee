//! TuiSink - Bridges ABK OutputEvents to TUI message channel
//!
//! Task 53: Implements the `OutputSink` trait so that structured events
//! emitted by the orchestration layer are forwarded directly to the
//! ratatui display via `mpsc::UnboundedSender<TuiMessage>`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use tokio::sync::mpsc;

use abk::orchestration::output::{OutputEvent, OutputSink, SharedSink};

use crate::app::TuiMessage;

/// Stream state machine constants (3-state lock-free state machine).
const STREAM_IDLE: u8 = 0;
const STREAM_REASONING: u8 = 1;
const STREAM_CONTENT: u8 = 2;

/// A sink that forwards ABK `OutputEvent`s to the TUI event channel.
///
/// Each event variant is mapped to an appropriate `TuiMessage` so the
/// ratatui render loop can display it in the output pane.
pub struct TuiSink {
    tx: mpsc::UnboundedSender<TuiMessage>,
    /// 3-state atomic: IDLE(0), REASONING(1), CONTENT(2).
    /// Tracks whether we're inside a reasoning or content stream so
    /// that transitions between states insert a blank separator line.
    stream_state: AtomicU8,
}

impl TuiSink {
    /// Create a new `TuiSink` wrapping the given channel sender.
    pub fn new(tx: mpsc::UnboundedSender<TuiMessage>) -> Self {
        Self { tx, stream_state: AtomicU8::new(STREAM_IDLE) }
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
                let prev = self.stream_state.swap(STREAM_CONTENT, Ordering::Relaxed);
                if prev != STREAM_CONTENT {
                    let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                }
                let _ = self.tx.send(TuiMessage::StreamDelta(delta));
                return; // delta path: skip the IDLE reset below
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
                context_tokens,
                tool_tokens,
            } => {
                let mode = if streaming { "Streaming" } else { "Non-streaming" };
                let total = context_tokens + tool_tokens;
                TuiMessage::OutputLine(format!(
                    "🔥 API Call {} | Ctx={}({}+{}) | {} | Model: {} | Tools: {}",
                    call_number, total, context_tokens, tool_tokens, mode, model, tool_count
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
                description,
            } => {
                // Intercept todowrite to update the todo panel
                if tool_name == "todowrite" && success {
                    let _ = self.tx.send(TuiMessage::TodoUpdate(content.clone()));
                }
                // Show compact one-liner with description when available
                match description {
                    Some(desc) => {
                        TuiMessage::OutputLine(format!("🔧 {} — {}", tool_name, desc))
                    }
                    None => {
                        let status = if success { "✓" } else { "✗" };
                        TuiMessage::OutputLine(format!("{} {}", status, tool_name))
                    }
                }
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
                let prev = self.stream_state.swap(STREAM_REASONING, Ordering::Relaxed);
                if prev != STREAM_REASONING {
                    let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                }
                let _ = self.tx.send(TuiMessage::ReasoningDelta(delta));
                return; // delta path: skip the IDLE reset below
            }
        };

        // Non-delta path: reset state to IDLE before sending the message.
        // This ensures the next delta chunk (reasoning or content) will
        // correctly insert a blank separator line.
        self.stream_state.store(STREAM_IDLE, Ordering::Relaxed);
        let _ = self.tx.send(msg);
    }
}
