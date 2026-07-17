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
  expectedTypes: Set<string>;
}

const DEFAULT_TIMEOUT_MS = 120_000;

export type ConnectionState = "connected" | "reconnecting" | "disconnected";

export class BackendSocket {
  private ws: ReconnectingWebSocket;
  private handlers = new Map<string, ((msg: ServerMessage) => void)[]>();
  private pendingByCallId = new Map<string, PendingRequest>();
  private pendingByRequestId = new Map<string, PendingRequest>();
  private pendingByType = new Map<string, PendingRequest>();
  private _closed = false;
  private _state: ConnectionState = "connected";
  private stateHandlers = new Set<(state: ConnectionState) => void>();

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
      this.emitState("reconnecting");
      this.rejectAll("WebSocket closed");
    });

    ws.addEventListener("open", () => {
      if (this._state === "reconnecting") {
        this._closed = false;
        this._state = "connected";
        this.emitState("connected");
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

  onStateChange(cb: (state: ConnectionState) => void): () => void {
    this.stateHandlers.add(cb);
    return () => this.stateHandlers.delete(cb);
  }

  send(msg: ClientMessage): void {
    if (this._closed) throw new Error("Backend disconnected");
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
        expectedTypes: new Set(["tool_call_result"]),
      });
      this.send(msg);
    });
  }

  requestById<T extends ServerMessage>(
    msg: ClientMessage & { request_id?: string },
    expectedTypes: string | string[],
    timeoutMs = 30_000
  ): Promise<T> {
    if (this._closed) {
      return Promise.reject(new Error("Backend disconnected"));
    }

    const requestId = msg.request_id || randomUUID();
    if (!requestId.trim() || Buffer.byteLength(requestId, "utf8") > 256) {
      return Promise.reject(
        new Error("request_id must be non-empty and at most 256 UTF-8 bytes")
      );
    }
    if (this.pendingByRequestId.has(requestId)) {
      return Promise.reject(new Error(`Duplicate request_id: ${requestId}`));
    }
    msg.request_id = requestId;
    const expected = new Set(
      Array.isArray(expectedTypes) ? expectedTypes : [expectedTypes]
    );

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingByRequestId.delete(requestId);
        reject(
          new Error(
            `Request ${requestId} timed out waiting for ${[...expected].join(" or ")}`
          )
        );
      }, timeoutMs);

      this.pendingByRequestId.set(requestId, {
        resolve: resolve as (msg: ServerMessage) => void,
        reject,
        timer,
        expectedTypes: expected,
      });

      try {
        this.send(msg);
      } catch (error) {
        clearTimeout(timer);
        this.pendingByRequestId.delete(requestId);
        reject(error);
      }
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
      if (this.pendingByType.has(expectedType)) {
        reject(
          new Error(
            `Only one uncorrelated ${expectedType} request may be in flight`
          )
        );
        return;
      }

      const entry: PendingRequest = {
        resolve: resolve as (msg: ServerMessage) => void,
        reject,
        timer: setTimeout(() => {
          this.pendingByType.delete(expectedType);
          reject(new Error(`Request timed out waiting for ${expectedType}`));
        }, timeoutMs),
        expectedTypes: new Set([expectedType]),
      };

      this.pendingByType.set(expectedType, entry);
      try {
        this.send(msg);
      } catch (error) {
        clearTimeout(entry.timer);
        this.pendingByType.delete(expectedType);
        reject(error);
      }
    });
  }

  on(type: string, handler: (msg: ServerMessage) => void): () => void {
    const list = this.handlers.get(type) || [];
    list.push(handler);
    this.handlers.set(type, list);
    return () => {
      const current = this.handlers.get(type);
      if (!current) return;
      const index = current.indexOf(handler);
      if (index !== -1) current.splice(index, 1);
      if (current.length === 0) this.handlers.delete(type);
    };
  }

  listenerCount(type?: string): number {
    if (type) return this.handlers.get(type)?.length || 0;
    let count = 0;
    for (const handlers of this.handlers.values()) count += handlers.length;
    return count;
  }

  pendingCount(): number {
    return (
      this.pendingByCallId.size +
      this.pendingByRequestId.size +
      this.pendingByType.size
    );
  }

  close(): void {
    // Guard: only close if not already in the fully-disconnected state.
    // Use AND so that a socket that is _closed=true but still reconnecting
    // (state=="reconnecting") is properly shut down.
    if (this._closed && this._state === "disconnected") return;
    this._closed = true;
    this._state = "disconnected";
    this.emitState("disconnected");
    this.rejectAll("WebSocket closed");
    this.ws.close();
  }

  private dispatch(msg: ServerMessage): void {
    let resolved = false;

    if ("call_id" in msg && typeof msg.call_id === "string") {
      const pending = this.pendingByCallId.get(msg.call_id);
      if (pending && pending.expectedTypes.has(msg.type)) {
        this.pendingByCallId.delete(msg.call_id);
        clearTimeout(pending.timer);
        pending.resolve(msg);
        resolved = true;
      }
    }

    if (
      "request_id" in msg &&
      typeof msg.request_id === "string" &&
      !resolved
    ) {
      const pending = this.pendingByRequestId.get(msg.request_id);
      if (pending && pending.expectedTypes.has(msg.type)) {
        this.pendingByRequestId.delete(msg.request_id);
        clearTimeout(pending.timer);
        pending.resolve(msg);
        resolved = true;
      }
    }

    if (!("request_id" in msg) && !resolved) {
      const candidates = [...this.pendingByRequestId.entries()].filter(([, pending]) =>
        pending.expectedTypes.has(msg.type)
      );
      if (candidates.length === 1) {
        const [requestId, pending] = candidates[0];
        this.pendingByRequestId.delete(requestId);
        clearTimeout(pending.timer);
        pending.reject(
          new Error(
            `Backend response ${msg.type} did not echo request_id; Mage v0.12.0 or newer is required`
          )
        );
        resolved = true;
      }
    }

    const pendingType = this.pendingByType.get(msg.type);
    if (pendingType && !resolved) {
      this.pendingByType.delete(msg.type);
      clearTimeout(pendingType.timer);
      pendingType.resolve(msg);
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

    for (const [, pending] of this.pendingByRequestId) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
    this.pendingByRequestId.clear();

    for (const [, pending] of this.pendingByType) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
    this.pendingByType.clear();
  }

  private emitState(state: ConnectionState): void {
    for (const handler of this.stateHandlers) handler(state);
  }
}
