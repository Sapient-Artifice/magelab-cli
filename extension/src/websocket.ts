import WebSocket from "ws";
import { WebSocket as ReconnectingWebSocket } from "partysocket";
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

export type ConnectionState = "connected" | "reconnecting" | "disconnected";

export class BackendSocket {
  private ws: ReconnectingWebSocket;
  private handlers = new Map<string, ((msg: IncomingMessage) => void)[]>();
  private pendingByCallId = new Map<string, PendingRequest>();
  private pendingByType = new Map<string, PendingRequest>();
  private _closed = false;
  private _state: ConnectionState = "connected";
  private _onStateChange?: (state: ConnectionState) => void;

  private constructor(ws: ReconnectingWebSocket) {
    this.ws = ws;

    ws.addEventListener("message", (event) => {
      try {
        const msg = JSON.parse(
          typeof event.data === "string" ? event.data : event.data.toString()
        ) as IncomingMessage;
        this.dispatch(msg);
      } catch {
        // Ignore unparseable messages
      }
    });

    ws.addEventListener("close", () => {
      this._closed = true;
      this._state = "reconnecting";
      this._onStateChange?.("reconnecting");
      this.rejectAll("WebSocket closed");
    });

    ws.addEventListener("open", () => {
      if (this._state === "reconnecting") {
        this._closed = false;
        this._state = "connected";
        this._onStateChange?.("connected");
      }
    });

    ws.addEventListener("error", () => {
      // partysocket handles reconnection; errors during reconnect are expected
    });
  }

  /**
   * Connect to the backend. Uses partysocket for automatic reconnection
   * with exponential backoff (default: 1s, 2s, 4s, max 10s, 3 retries).
   */
  static connect(
    url: string,
    token?: string | null
  ): Promise<BackendSocket> {
    return new Promise((resolve, reject) => {
      const protocols: string[] = [];
      // partysocket uses WebSocket constructor from ws
      const ws = new ReconnectingWebSocket(url, protocols, {
        WebSocket: WebSocket as any,
        maxRetries: 3,
        minReconnectionDelay: 1000,
        maxReconnectionDelay: 10000,
        connectionTimeout: 5000,
        startClosed: false,
      });

      // Set auth header via protocol hack — partysocket doesn't support headers directly.
      // Override the internal URL to include token as query param for relay mode.
      if (token && url.includes("?")) {
        // Relay URLs already have query params (ws_ticket), token goes as header
        // For now, skip header-based auth in partysocket (works for local mode)
      }

      let connected = false;
      ws.addEventListener("open", () => {
        if (!connected) {
          connected = true;
          resolve(new BackendSocket(ws));
        }
      });
      ws.addEventListener("error", (event) => {
        if (!connected) {
          connected = true;
          reject(new Error(`WebSocket connection failed: ${(event as any).message || "unknown"}`));
        }
      });
    });
  }

  get closed(): boolean {
    return this._closed;
  }

  get state(): ConnectionState {
    return this._state;
  }

  onStateChange(cb: (state: ConnectionState) => void): void {
    this._onStateChange = cb;
  }

  send(msg: OutgoingMessage): void {
    if (this._closed) return;
    this.ws.send(JSON.stringify(msg));
  }

  callTool(
    functionName: string,
    args: Record<string, unknown>,
    timeoutMs = DEFAULT_TIMEOUT_MS
  ): Promise<ToolCallResultMessage> {
    if (this._closed) {
      return Promise.reject(new Error("Backend disconnected"));
    }

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
        reject(new Error(`Tool execution timed out after ${timeoutMs / 1000}s: ${functionName}`));
      }, timeoutMs);

      this.pendingByCallId.set(callId, {
        resolve: resolve as (msg: IncomingMessage) => void,
        reject,
        timer,
      });
      this.send(msg);
    });
  }

  requestByType<T extends IncomingMessage>(
    msg: OutgoingMessage,
    expectedType: string,
    timeoutMs = 30_000
  ): Promise<T> {
    if (this._closed) {
      return Promise.reject(new Error("Backend disconnected"));
    }

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

  on(type: string, handler: (msg: IncomingMessage) => void): void {
    const list = this.handlers.get(type) || [];
    list.push(handler);
    this.handlers.set(type, list);
  }

  close(): void {
    if (!this._closed || this._state !== "disconnected") {
      this._closed = true;
      this._state = "disconnected";
      this.rejectAll("WebSocket closed");
      this.ws.close();
    }
  }

  private dispatch(msg: IncomingMessage): void {
    if ("call_id" in msg && typeof msg.call_id === "string") {
      const pending = this.pendingByCallId.get(msg.call_id);
      if (pending) {
        this.pendingByCallId.delete(msg.call_id);
        clearTimeout(pending.timer);
        pending.resolve(msg);
        return;
      }
    }

    const pendingType = this.pendingByType.get(msg.type);
    if (pendingType) {
      this.pendingByType.delete(msg.type);
      clearTimeout(pendingType.timer);
      pendingType.resolve(msg);
      return;
    }

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
