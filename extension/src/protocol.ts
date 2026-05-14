// AUTO-GENERATED from schemas/websocket/protocol.json
// Do not edit manually. Run: npx tsx schemas/codegen.ts

export interface TextMessage {
  type: "text";
  text: string;
}

export interface AudioMessage {
  type: "audio";
}

export interface GetRuntimeConfig {
  type: "get_runtime_config";
}

export interface GetTools {
  type: "get_tools";
}

export interface ToolCallRequest {
  type: "tool_call";
  call_id: string;
  function_name: string;
  arguments?: Record<string, unknown>;
}

export interface ConfirmationResponse {
  type: "confirmation_response";
  confirmation_id: string;
  confirmed: boolean;
  remember: boolean;
}

export interface SetModel {
  type: "set_model";
  model: string;
}

export interface SetVoice {
  type: "set_voice";
  voice: string;
}

export interface GetModels {
  type: "get_models";
  endpoint: string;
  api_key: string;
}

export interface NewChat {
  type: "new_chat";
}

export interface GetChats {
  type: "get_chats";
}

export interface SetChat {
  type: "set_chat";
  history_path: string;
}

export interface Control {
  type: "control";
  action: string;
}

export interface Lifecycle {
  type: "lifecycle";
  action?: string;
}

export interface CancelSubagent {
  type: "cancel_subagent";
  task_id: string;
}

export interface AssistantStream {
  type: "assistant_stream";
  phase: string;
  token?: string;
  text?: string;
  content?: string;
  model?: string;
  stream_id?: string;
}

export interface Assistant {
  type: "assistant";
  text?: string;
}

export interface AssistantComplete {
  type: "assistant_complete";
}

export interface Transcription {
  type: "transcription";
  text?: string;
}

export interface ConfirmationRequest {
  type: "confirmation_request";
  confirmation_id: string;
  function_name: string;
  script?: string;
  arguments?: Record<string, unknown>;
}

export interface ToolResult {
  type: "tool_result";
  function_name: string;
  result?: unknown;
}

export interface ToolDebug {
  type: "tool_debug";
  message_type?: string;
  content?: string;
}

export interface RuntimeConfig {
  type: "runtime_config";
  llm_provider_name?: string;
  llm_endpoint?: string;
  llm_model_name?: string;
  voice_model?: string;
  tts_stream?: boolean;
  mute?: boolean;
  history_path?: string;
  list_files?: string[];
}

export interface ToolsList {
  type: "tools_list";
  tools: Record<string, unknown>[];
}

export interface ToolCallResult {
  type: "tool_call_result";
  call_id: string;
  success: boolean;
  result?: unknown;
  error?: string;
}

export interface SetModelResult {
  type: "set_model_result";
  success: boolean;
  model?: string;
}

export interface ModelsResult {
  type: "models_result";
  models?: string[];
}

export interface NewChatResult {
  type: "new_chat_result";
  history_path?: string;
}

export interface ChatListResult {
  type: "chat_list_result";
  ok?: boolean;
  history_path?: string;
  chats?: string[];
}

export interface ChatSwitchResult {
  type: "chat_switch_result";
  ok?: boolean;
  history_path?: string;
  error?: string;
}

export interface TokenCount {
  type: "token_count";
  sys_count?: number;
  win_count?: number;
  total_count?: number;
}

export interface SubagentUpdate {
  type: "subagent_update";
  task_id: string;
  name: string;
  status?: string;
  progress?: string;
  output_preview?: string;
}

export interface SubagentComplete {
  type: "subagent_complete";
  task_id: string;
  name: string;
  status?: string;
  result?: string;
  error?: string;
}

export interface Notify {
  type: "notify";
  title: string;
  body: string;
}

export interface OpenUrl {
  type: "open_url";
  url: string;
}

export interface OpenFile {
  type: "open_file";
  filepath: string;
}

export interface BrokerStatus {
  type: "broker_status";
  devices?: string[];
  bound_device_id?: string;
  connected?: boolean;
}

export interface BindResult {
  type: "bind_result";
  bound_device_id?: string;
}

export interface BrokerError {
  type: "broker_error";
  code?: string;
  message?: string;
}

export interface ErrorMessage {
  type: "error";
  message?: string;
}

export interface Ping {
  type: "ping";
}

export type ClientMessage =
  | TextMessage
  | AudioMessage
  | GetRuntimeConfig
  | GetTools
  | ToolCallRequest
  | ConfirmationResponse
  | SetModel
  | SetVoice
  | GetModels
  | NewChat
  | GetChats
  | SetChat
  | Control
  | Lifecycle
  | CancelSubagent;

export type ServerMessage =
  | AssistantStream
  | Assistant
  | AssistantComplete
  | Transcription
  | ConfirmationRequest
  | ToolResult
  | ToolDebug
  | RuntimeConfig
  | ToolsList
  | ToolCallResult
  | SetModelResult
  | ModelsResult
  | NewChatResult
  | ChatListResult
  | ChatSwitchResult
  | TokenCount
  | SubagentUpdate
  | SubagentComplete
  | Notify
  | OpenUrl
  | OpenFile
  | BrokerStatus
  | BindResult
  | BrokerError
  | ErrorMessage
  | Ping;
