/**
 * A minimal WebSocket server that implements the same protocol as
 * the MageLab Python backend. Used for integration tests without
 * needing the real backend running.
 *
 * Handles: get_tools, tool_call, confirmation_response
 */
import { WebSocketServer, WebSocket } from "ws";

export interface MockBackendOptions {
  /** Tool schemas to return from get_tools */
  tools?: any[];
  /** If set, tool_call sends a confirmation_request before completing */
  requireConfirmation?: boolean;
  /** Custom tool_call handler — return the result string */
  onToolCall?: (name: string, args: any) => string | Promise<string>;
  /** Delay before sending tool_call_result (ms) */
  toolDelay?: number;
  /** Intercept a message. Return true when the default handler should not run. */
  onMessage?: (ws: WebSocket, data: any) => boolean | Promise<boolean>;
  /** Custom assistant-turn script. */
  onText?: (ws: WebSocket, data: any) => void | Promise<void>;
}

const DEFAULT_TOOLS = [
  {
    type: "function",
    function: {
      name: "run_python",
      description: "Execute Python code in an isolated interpreter",
      parameters: {
        type: "object",
        properties: {
          code: { type: "string", description: "Python code to execute" },
        },
        required: ["code"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "search_web",
      description: "Search the web using Brave Search",
      parameters: {
        type: "object",
        properties: {
          query: { type: "string", description: "Search query" },
          num_results: { type: "integer", description: "Number of results", minimum: 1, maximum: 20 },
        },
        required: ["query"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "read_file",
      description: "Read a file from disk",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string", description: "Absolute path" },
        },
        required: ["path"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "write_file",
      description: "Write content to a file",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string" },
          content: { type: "string" },
        },
        required: ["path", "content"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "run_bash",
      description: "Run a shell command",
      parameters: {
        type: "object",
        properties: {
          command: { type: "string", description: "Shell command" },
        },
        required: ["command"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "open_file",
      description: "Open a file in the editor",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string" },
        },
        required: ["path"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "generate_image",
      description: "Generate an image from a text prompt",
      parameters: {
        type: "object",
        properties: {
          prompt: { type: "string", description: "Image description" },
          style: { type: "string", enum: ["natural", "vivid"] },
          width: { type: "number" },
          height: { type: "number" },
        },
        required: ["prompt"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "calculate",
      description: "Evaluate a math expression",
      parameters: {
        type: "object",
        properties: {
          expression: { type: "string" },
        },
        required: ["expression"],
      },
    },
  },
];

export class MockBackend {
  private server: WebSocketServer;
  private options: MockBackendOptions;
  private _port = 0;
  private pendingConfirmations = new Map<string, { callId: string; ws: WebSocket }>();
  private _receivedMessages: any[] = [];
  private nextChatId = 100;
  private activeTurnId: string | undefined;

  constructor(options: MockBackendOptions = {}) {
    this.options = options;
    this.server = new WebSocketServer({ port: 0 });

    this.server.on("connection", (ws) => {
      ws.on("message", (raw) => {
        const data = JSON.parse(raw.toString());
        this.handleMessage(ws, data);
      });
    });
  }

  get port(): number {
    return this._port;
  }

  get url(): string {
    return `ws://127.0.0.1:${this._port}`;
  }

  get receivedMessages(): readonly any[] {
    return this._receivedMessages;
  }

  async start(): Promise<void> {
    return new Promise((resolve) => {
      this.server.on("listening", () => {
        this._port = (this.server.address() as { port: number }).port;
        resolve();
      });
    });
  }

  async close(): Promise<void> {
    return new Promise((resolve) => {
      this.server.close(() => resolve());
    });
  }

  private async handleMessage(ws: WebSocket, data: any) {
    this._receivedMessages.push(data);
    if (await this.options.onMessage?.(ws, data)) return;

    switch (data.type) {
      case "get_tools":
        ws.send(JSON.stringify({
          type: "tools_list",
          tools: this.options.tools ?? DEFAULT_TOOLS,
        }));
        break;

      case "tool_call":
        await this.handleToolCall(ws, data);
        break;

      case "confirmation_response":
        this.handleConfirmationResponse(ws, data);
        break;

      case "get_runtime_config":
        ws.send(JSON.stringify({
          type: "runtime_config",
          llm_provider_name: "test",
          llm_model_name: "test-model",
          voice_model: "test-voice",
          tts_stream: false,
          mute: true,
          history_path: "/tmp/test",
          list_files: [],
        }));
        break;

      case "write_runtime_state":
        ws.send(JSON.stringify({
          type: "runtime_state_write_result",
          request_id: data.request_id,
          ok: true,
          applied: data.state,
          snapshot: data.state,
        }));
        break;

      case "new_chat":
        ws.send(JSON.stringify({
          type: "new_chat_result",
          request_id: data.request_id,
          ok: true,
          chat_id: this.nextChatId++,
          chat_records: [],
        }));
        break;

      case "set_chat":
        ws.send(JSON.stringify({
          type: "chat_switch_result",
          request_id: data.request_id,
          ok: true,
          chat_id: data.chat_id,
          chat_records: [],
        }));
        break;

      case "text":
        this.activeTurnId = data.client_request_id;
        if (this.options.onText) {
          await this.options.onText(ws, data);
        } else {
          ws.send(JSON.stringify({
            type: "assistant_stream",
            phase: "start",
            client_request_id: data.client_request_id,
          }));
          ws.send(JSON.stringify({
            type: "assistant_stream",
            phase: "delta",
            token: `answer:${data.text}`,
            client_request_id: data.client_request_id,
          }));
          ws.send(JSON.stringify({
            type: "assistant_stream",
            phase: "end",
            client_request_id: data.client_request_id,
          }));
          ws.send(JSON.stringify({
            type: "assistant_complete",
            client_request_id: data.client_request_id,
            status: "completed",
          }));
        }
        break;

      case "control":
        if (data.action === "stop" && this.activeTurnId) {
          ws.send(JSON.stringify({
            type: "assistant_complete",
            client_request_id: this.activeTurnId,
            status: "cancelled",
          }));
          this.activeTurnId = undefined;
        }
        break;
    }
  }

  private async handleToolCall(ws: WebSocket, data: any) {
    const { call_id, function_name, arguments: args } = data;

    if (this.options.requireConfirmation) {
      const confirmId = `conf-${call_id}`;
      this.pendingConfirmations.set(confirmId, { callId: call_id, ws });

      ws.send(JSON.stringify({
        type: "confirmation_request",
        confirmation_id: confirmId,
        function_name,
        script: JSON.stringify(args),
        arguments: args ?? {},
      }));
      return; // Wait for confirmation_response
    }

    await this.executeAndRespond(ws, call_id, function_name, args);
  }

  private handleConfirmationResponse(ws: WebSocket, data: any) {
    const pending = this.pendingConfirmations.get(data.confirmation_id);
    if (!pending) return;

    this.pendingConfirmations.delete(data.confirmation_id);

    if (data.confirmed) {
      this.executeAndRespond(pending.ws, pending.callId, "confirmed_tool", {});
    } else {
      pending.ws.send(JSON.stringify({
        type: "tool_call_result",
        call_id: pending.callId,
        success: false,
        error: "User denied tool execution",
      }));
    }
  }

  private async executeAndRespond(
    ws: WebSocket,
    callId: string,
    functionName: string,
    args: any
  ) {
    const delay = this.options.toolDelay ?? 0;
    if (delay > 0) {
      await new Promise((r) => setTimeout(r, delay));
    }

    try {
      let result: string;
      if (this.options.onToolCall) {
        result = await this.options.onToolCall(functionName, args);
      } else {
        result = `executed ${functionName}(${JSON.stringify(args)})`;
      }

      ws.send(JSON.stringify({
        type: "tool_call_result",
        call_id: callId,
        success: true,
        result,
      }));
    } catch (err: any) {
      ws.send(JSON.stringify({
        type: "tool_call_result",
        call_id: callId,
        success: false,
        error: err.message,
      }));
    }
  }
}
