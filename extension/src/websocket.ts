import WebSocket from "ws";
import { WebSocket as ReconnectingWebSocket } from "partysocket";
import { randomUUID } from "node:crypto";
import type {
  ClientMessage,
  ServerMessage,
  GetTools,
  ToolCallRequest,
  ConfirmationResponse,
  ToolCallResult,
  ToolsList,
  ConfirmationRequest,
  ErrorMessage,
} from "./protocol.js";

// Re-export protocol types used by other modules
export type { ConfirmationRequest, ToolCallResult, ToolsList, ServerMessage, ClientMessage };

// ToolSchema is a nested shape within ToolsList — not in the protocol schema
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
  resolve: (msg: ServerMessage) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const DEFAULT_TIMEOUT_MS = 120_000;

export type ConnectionState = "connected" | "reconnecting" | "disconnected";

export class BackendSocket {
  private ws: ReconnectingWebSocket;
  private handlers = new Map<string, ((msg: ServerMessage) => void)[]>();
  private pendingByCallId = new Map<string, PendingRequest>();
  private pendingByType = new Map<string, PendingRequest[]>();
  private _closed = false;
  private _state: ConnectionState = "connected";
  private _onStateChange?: (state: ConnectionState) => void;

  private constructor(ws: ReconnectingWebSocket) {
    this.ws = ws;

    ws.addEventListener("message", (event) => {
      try {
        const msg = JSON.parse(
          typeof event.data === "string" ? event.data : event.data.toString()
        ) as ServerMessage;
        this.dispatch(msg);
      } catch {
        // Ignore unparseable messages
      }
    });

    ws.addEventListener("close", () => {
      // Don't fire reconnecting transition if already explicitly closed
      if (this._state === "disconnected") return;
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
    rawUrl: string,
    token?: string | null
  ): Promise<BackendSocket> {
    // Append token as a query parameter before opening the socket.
    // partysocket does not support custom HTTP headers on the upgrade request,
    // so ?token=<jwt> is the only portable mechanism.
    let url = rawUrl;
    if (token) {
      const sep = url.includes("?") ? "&" : "?";
      url = `${url}${sep}token=${encodeURIComponent(token)}`;
    }

    return new Promise((resolve, reject) => {
      const protocols: string[] = [];
      const ws = new ReconnectingWebSocket(url, protocols, {
        WebSocket: WebSocket as any,
        maxRetries: 3,
        minReconnectionDelay: 1000,
        maxReconnectionDelay: 10000,
        connectionTimeout: 5000,
        startClosed: false,
      });

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

  send(msg: ClientMessage): void {
    if (this._closed) return;
    this.ws.send(JSON.stringify(msg));
  }

  callTool(
    functionName: string,
    args: Record<string, unknown>,
    timeoutMs = DEFAULT_TIMEOUT_MS
  ): Promise<ToolCallResult> {
    if (this._closed) {
      return Promise.reject(new Error("Backend disconnected"));
    }

    const callId = randomUUID();
    const msg: ToolCallRequest = {
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
        resolve: resolve as (msg: ServerMessage) => void,
        reject,
        timer,
      });
      this.send(msg);
    });
  }

  requestByType<T extends ServerMessage>(
    msg: ClientMessage,
    expectedType: string,
    timeoutMs = 30_000
  ): Promise<T> {
    if (this._closed) {
      return Promise.reject(new Error("Backend disconnected"));
    }

    return new Promise((resolve, reject) => {
      const entry: PendingRequest = {
        resolve: resolve as (msg: ServerMessage) => void,
        reject,
        timer: setTimeout(() => {
          const queue = this.pendingByType.get(expectedType);
          if (queue) {
            const idx = queue.indexOf(entry);
            if (idx !== -1) queue.splice(idx, 1);
            if (queue.length === 0) this.pendingByType.delete(expectedType);
          }
          reject(new Error(`Request timed out waiting for ${expectedType}`));
        }, timeoutMs),
      };

      const queue = this.pendingByType.get(expectedType) || [];
      queue.push(entry);
      this.pendingByType.set(expectedType, queue);
      this.send(msg);
    });
  }

  on(type: string, handler: (msg: ServerMessage) => void): void {
    const list = this.handlers.get(type) || [];
    list.push(handler);
    this.handlers.set(type, list);
  }

  close(): void {
    // Guard: only close if not already in the fully-disconnected state.
    // Use AND so that a socket that is _closed=true but still reconnecting
    // (state=="reconnecting") is properly shut down.
    if (this._closed && this._state === "disconnected") return;
    this._closed = true;
    this._state = "disconnected";
    this.rejectAll("WebSocket closed");
    this.ws.close();
  }

  private dispatch(msg: ServerMessage): void {
    if ("call_id" in msg && typeof msg.call_id === "string") {
      const pending = this.pendingByCallId.get(msg.call_id);
      if (pending) {
        this.pendingByCallId.delete(msg.call_id);
        clearTimeout(pending.timer);
        pending.resolve(msg);
        return;
      }
    }

    const typeQueue = this.pendingByType.get(msg.type);
    if (typeQueue && typeQueue.length > 0) {
      const pendingType = typeQueue.shift()!;
      if (typeQueue.length === 0) this.pendingByType.delete(msg.type);
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

    for (const [, queue] of this.pendingByType) {
      for (const pending of queue) {
        clearTimeout(pending.timer);
        pending.reject(new Error(reason));
      }
    }
    this.pendingByType.clear();
  }
}
