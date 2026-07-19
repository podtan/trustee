//! Shared server state: wraps `Session` and a broadcast channel.

use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Mutex};
use trustee_core::session::Session;
use trustee_core::types::TuiMessage;

/// Shared state accessible by all axum handlers.
#[derive(Clone)]
pub struct ServerState {
    /// The agent session, protected by a mutex.
    pub session: Arc<Mutex<Session>>,
    /// Broadcast sender for WebSocket fan-out.
    /// Messages are JSON-serialized `TuiMessage` strings.
    pub ws_tx: broadcast::Sender<String>,
}

impl ServerState {
    /// Create new shared state from a session and broadcast sender.
    pub fn new(session: Session, ws_tx: broadcast::Sender<String>) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            ws_tx,
        }
    }

    /// Spawn a background task that owns the workflow receiver and broadcasts
    /// each message to all WebSocket subscribers.
    ///
    /// The receiver is moved into the task — no locking needed to await it.
    /// When a message arrives, the task briefly locks the session to call
    /// `handle_workflow_message`, then broadcasts the JSON to WebSocket clients.
    pub fn spawn_drain_task(self, mut workflow_rx: mpsc::UnboundedReceiver<TuiMessage>) {
        tokio::spawn(async move {
            while let Some(msg) = workflow_rx.recv().await {
                // Process the message through Session's handler (updates state)
                {
                    let mut session = self.session.lock().await;
                    session.handle_workflow_message(msg.clone());
                }

                // Broadcast the raw message to WebSocket clients
                let json = serde_json::to_string(&SerializableMessage(&msg)).unwrap_or_default();
                let _ = self.ws_tx.send(json);
            }
        });
    }
}

/// Wrapper to serialize `TuiMessage` as JSON with a `type` discriminator.
struct SerializableMessage<'a>(&'a TuiMessage);

impl<'a> serde::Serialize for SerializableMessage<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        match self.0 {
            TuiMessage::OutputLine(line) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "OutputLine")?;
                s.serialize_field("line", line)?;
                s.end()
            }
            TuiMessage::StreamDelta(delta) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "StreamDelta")?;
                s.serialize_field("delta", delta)?;
                s.end()
            }
            TuiMessage::ReasoningDelta(delta) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "ReasoningDelta")?;
                s.serialize_field("delta", delta)?;
                s.end()
            }
            TuiMessage::WorkflowCompleted => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "WorkflowCompleted")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::WorkflowError(err) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "WorkflowError")?;
                s.serialize_field("error", err)?;
                s.end()
            }
            TuiMessage::ResumeInfo(_) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "ResumeInfo")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::TodoUpdate(content) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "TodoUpdate")?;
                s.serialize_field("content", content)?;
                s.end()
            }
            TuiMessage::WorkflowCancelled => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "WorkflowCancelled")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::HandoffReady(_) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "HandoffReady")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::ToolPending { tool_name, hint } => {
                let mut s = serializer.serialize_struct("msg", 3)?;
                s.serialize_field("type", "ToolPending")?;
                s.serialize_field("tool_name", tool_name)?;
                s.serialize_field("hint", hint)?;
                s.end()
            }
            TuiMessage::ToolDone { tool_name, success, hint } => {
                let mut s = serializer.serialize_struct("msg", 4)?;
                s.serialize_field("type", "ToolDone")?;
                s.serialize_field("tool_name", tool_name)?;
                s.serialize_field("success", success)?;
                s.serialize_field("hint", hint)?;
                s.end()
            }
            TuiMessage::ContextTokensUpdated(count) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "ContextTokensUpdated")?;
                s.serialize_field("count", count)?;
                s.end()
            }
            TuiMessage::McpServerStatus { name, connected, tool_count, error } => {
                let mut s = serializer.serialize_struct("msg", 5)?;
                s.serialize_field("type", "McpServerStatus")?;
                s.serialize_field("name", name)?;
                s.serialize_field("connected", connected)?;
                s.serialize_field("tool_count", tool_count)?;
                s.serialize_field("error", error)?;
                s.end()
            }
        }
    }
}
