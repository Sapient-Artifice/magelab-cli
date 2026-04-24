import WebSocket from "ws";
import { randomUUID } from "node:crypto";

// -- Outgoing messages (client -> backend) --

export interface GetToolsMessage {
  type: "get_tools";
}

export interface ToolCallMessage {
  type: "tool_call";
  call_id: string;
  function_name: string;
  arguments: Record<string, unknown>;
}

export interface ConfirmationResponseMessage {
  type: "confirmation_response";
  confirmation_id: string;
  confirmed: boolean;
  remember: boolean;
}

export type OutgoingMessage =
  | GetToolsMessage
  | ToolCallMessage
  | ConfirmationResponseMessage;

// -- Incoming messages (backend -> client) --

export interface ToolsListMessage {
  type: "tools_list";
  tools: ToolSchema[];
}

export interface ToolCallResultMessage {
  type: "tool_call_result";
  call_id: string;
  success: boolean;
  result?: unknown;
  error?: string;
}

export interface ConfirmationRequestMessage {
  type: "confirmation_request";
  confirmation_id: string;
  function_name: string;
  script?: string;
  arguments?: Record<string, unknown>;
}

export interface ErrorMessage {
  type: "error";
  message?: string;
}

export type IncomingMessage =
  | ToolsListMessage
  | ToolCallResultMessage
  | ConfirmationRequestMessage
  | ErrorMessage
  | { type: string; [key: string]: unknown };

// -- Tool schema (OpenAI function-calling format) --

export interface ToolSchema {
  type: "function";
  function: {
    name: string;
    description?: string;
    parameters?: {
      type: "object";
      properties?: Record<string, unknown>;
      required?: string[];
    };
  };
}

// -- Pending request tracker --

interface PendingRequest {
  resolve: (msg: IncomingMessage) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const DEFAULT_TIMEOUT_MS = 120_000;

export class BackendSocket {
  private ws: WebSocket;
  private handlers = new Map<string, ((msg: IncomingMessage) => void)[]>();
  private pendingByCallId = new Map<string, PendingRequest>();
  private pendingByType = new Map<string, PendingRequest>();
  private _closed = false;

  private constructor(ws: WebSocket) {
    this.ws = ws;
    ws.on("message", (data) => {
      try {
        const msg = JSON.parse(data.toString()) as IncomingMessage;
        this.dispatch(msg);
      } catch {
        // Ignore unparseable messages
      }
    });
    ws.on("close", () => {
      this._closed = true;
      this.rejectAll("WebSocket closed");
    });
    ws.on("error", () => {
      this._closed = true;
      this.rejectAll("WebSocket error");
    });
  }

  static connect(
    url: string,
    token?: string | null
  ): Promise<BackendSocket> {
    return new Promise((resolve, reject) => {
      const headers: Record<string, string> = {};
      if (token) {
        headers["Authorization"] = `Bearer ${token}`;
      }
      const ws = new WebSocket(url, { headers });
      ws.on("open", () => resolve(new BackendSocket(ws)));
      ws.on("error", (err) =>
        reject(new Error(`WebSocket connection failed: ${err.message}`))
      );
    });
  }

  get closed(): boolean {
    return this._closed;
  }

  send(msg: OutgoingMessage): void {
    this.ws.send(JSON.stringify(msg));
  }

  /**
   * Send a message and await a response correlated by call_id.
   */
  callTool(
    functionName: string,
    args: Record<string, unknown>,
    timeoutMs = DEFAULT_TIMEOUT_MS
  ): Promise<ToolCallResultMessage> {
    const callId = randomUUID();
    const msg: ToolCallMessage = {
      type: "tool_call",
      call_id: callId,
      function_name: functionName,
      arguments: args,
    };

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingByCallId.delete(callId);
        reject(
          new Error(
            `Tool execution timed out after ${timeoutMs / 1000}s: ${functionName}`
          )
        );
      }, timeoutMs);

      this.pendingByCallId.set(callId, {
        resolve: resolve as (msg: IncomingMessage) => void,
        reject,
        timer,
      });
      this.send(msg);
    });
  }

  /**
   * Send a message and await a response matched by message type.
   * Used for simple request/response pairs like get_tools -> tools_list.
   */
  requestByType<T extends IncomingMessage>(
    msg: OutgoingMessage,
    expectedType: string,
    timeoutMs = 30_000
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingByType.delete(expectedType);
        reject(new Error(`Request timed out waiting for ${expectedType}`));
      }, timeoutMs);

      this.pendingByType.set(expectedType, {
        resolve: resolve as (msg: IncomingMessage) => void,
        reject,
        timer,
      });
      this.send(msg);
    });
  }

  /**
   * Register a persistent handler for a specific message type.
   */
  on(type: string, handler: (msg: IncomingMessage) => void): void {
    const list = this.handlers.get(type) || [];
    list.push(handler);
    this.handlers.set(type, list);
  }

  close(): void {
    if (!this._closed) {
      this._closed = true;
      this.ws.close(1000);
    }
  }

  private dispatch(msg: IncomingMessage): void {
    // Check call_id correlation first (for tool_call_result)
    if ("call_id" in msg && typeof msg.call_id === "string") {
      const pending = this.pendingByCallId.get(msg.call_id);
      if (pending) {
        this.pendingByCallId.delete(msg.call_id);
        clearTimeout(pending.timer);
        pending.resolve(msg);
        return;
      }
    }

    // Check type-based correlation (for tools_list, etc.)
    const pendingType = this.pendingByType.get(msg.type);
    if (pendingType) {
      this.pendingByType.delete(msg.type);
      clearTimeout(pendingType.timer);
      pendingType.resolve(msg);
      return;
    }

    // Dispatch to persistent handlers
    const handlers = this.handlers.get(msg.type);
    if (handlers) {
      for (const h of handlers) {
        h(msg);
      }
    }
  }

  private rejectAll(reason: string): void {
    for (const [, pending] of this.pendingByCallId) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
    this.pendingByCallId.clear();

    for (const [, pending] of this.pendingByType) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
    this.pendingByType.clear();
  }
}
