use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Messages sent from CLI to backend via WebSocket
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum OutgoingMessage {
    /// Send a text chat message (backend expects type:"text", field:"text")
    #[serde(rename = "text")]
    Chat { text: String },

    #[serde(rename = "confirmation_response")]
    ConfirmationResponse {
        confirmation_id: String,
        confirmed: bool,
        remember: bool,
    },

    #[serde(rename = "get_runtime_config")]
    GetRuntimeConfig,

    #[serde(rename = "set_model")]
    SetModel { model: String },

    #[serde(rename = "set_voice")]
    SetVoice { voice: String },

    #[serde(rename = "get_models")]
    GetModels { endpoint: String, api_key: String },

    #[serde(rename = "new_chat")]
    NewChat,

    #[serde(rename = "get_chats")]
    GetChats,

    #[serde(rename = "set_chat")]
    SetChat { history_path: String },

    #[serde(rename = "control")]
    Control { action: String },

    #[serde(rename = "get_tools")]
    GetTools,

    #[serde(rename = "tool_call")]
    ToolCall {
        call_id: String,
        function_name: String,
        arguments: HashMap<String, Value>,
    },
}

/// Messages received from backend via WebSocket
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum IncomingMessage {
    /// Streaming response: phase="start"|"delta"|"end", with stream_id
    #[serde(rename = "assistant_stream")]
    AssistantStream {
        phase: String,
        #[serde(default)]
        token: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        stream_id: Option<String>,
    },

    /// Non-streaming complete response
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        text: Option<String>,
    },

    /// Signals the assistant response is fully complete
    #[serde(rename = "assistant_complete")]
    AssistantComplete {},

    /// Echo of the user's transcribed/typed text
    #[serde(rename = "transcription")]
    Transcription {
        #[serde(default)]
        text: Option<String>,
    },

    #[serde(rename = "confirmation_request")]
    ConfirmationRequest {
        confirmation_id: String,
        function_name: String,
        #[serde(default)]
        script: Option<String>,
        #[serde(default)]
        arguments: HashMap<String, Value>,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        function_name: String,
        #[serde(default)]
        result: Option<Value>,
    },

    #[serde(rename = "tool_debug")]
    ToolDebug {
        #[serde(default)]
        message_type: Option<String>,
        #[serde(default)]
        content: Option<String>,
    },

    #[serde(rename = "runtime_config")]
    RuntimeConfig(HashMap<String, Value>),

    #[serde(rename = "set_model_result")]
    SetModelResult {
        success: bool,
        #[serde(default)]
        model: Option<String>,
    },

    #[serde(rename = "models_result")]
    ModelsResult {
        #[serde(default)]
        models: Vec<String>,
    },

    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        message: Option<String>,
    },

    #[serde(rename = "new_chat_result")]
    NewChatResult {
        #[serde(default)]
        history_path: Option<String>,
    },

    #[serde(rename = "chat_list_result")]
    ChatListResult {
        #[serde(default)]
        ok: bool,
        #[serde(default)]
        history_path: Option<String>,
        #[serde(default)]
        chats: Vec<String>,
    },

    #[serde(rename = "chat_switch_result")]
    ChatSwitchResult {
        #[serde(default)]
        ok: bool,
        #[serde(default)]
        history_path: Option<String>,
        #[serde(default)]
        error: Option<String>,
    },

    #[serde(rename = "notify")]
    Notify { title: String, body: String },

    #[serde(rename = "open_url")]
    OpenUrl { url: String },

    #[serde(rename = "open_file")]
    OpenFile { filepath: String },

    #[serde(rename = "token_count")]
    TokenCount {
        #[serde(default)]
        sys_count: i64,
        #[serde(default)]
        win_count: i64,
        #[serde(default)]
        total_count: i64,
    },

    #[serde(rename = "subagent_update")]
    SubagentUpdate {
        task_id: String,
        name: String,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        progress: Option<String>,
        #[serde(default)]
        output_preview: Option<String>,
    },

    #[serde(rename = "subagent_complete")]
    SubagentComplete {
        task_id: String,
        name: String,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        error: Option<String>,
    },

    #[serde(rename = "broker_status")]
    BrokerStatus {
        #[serde(default)]
        devices: Vec<String>,
        #[serde(default)]
        bound_device_id: Option<String>,
        #[serde(default)]
        connected: bool,
    },

    #[serde(rename = "bind_result")]
    BindResult {
        #[serde(default)]
        bound_device_id: Option<String>,
    },

    #[serde(rename = "broker_error")]
    BrokerError {
        #[serde(default)]
        code: String,
        #[serde(default)]
        message: String,
    },

    #[serde(rename = "tools_list")]
    ToolsList {
        #[serde(default)]
        tools: Vec<Value>,
    },

    #[serde(rename = "tool_call_result")]
    ToolCallResult {
        call_id: String,
        #[serde(default)]
        success: bool,
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        error: Option<String>,
    },

    #[serde(rename = "ping")]
    Ping {},

    #[serde(other)]
    Unknown,
}
