import { randomUUID } from "node:crypto";
import type {
  Assistant,
  AssistantComplete,
  AssistantStream,
  ChatSwitchResult,
  NewChatResult,
  RuntimeStateWriteResult,
  ServerMessage,
} from "../protocol.js";
import { BackendSocket } from "../websocket.js";
import {
  MageConnectionError,
  MageConnectionLostError,
  MagePartialFailureError,
  MageProtocolError,
  MageSetupError,
  MageTimeoutError,
  MageValidationError,
} from "./errors.js";
import {
  SerialMutationCoordinator,
  type MutationCoordinator,
  type MutationLease,
} from "./coordinator.js";

export * from "./errors.js";
export * from "./coordinator.js";

export interface McpState {
  enabled_servers: string[];
}

export interface SessionState {
  active_chat_id?: number;
  provider?: Record<string, unknown>;
  prompt?: Record<string, unknown>;
  tools?: Record<string, unknown>;
  general?: Record<string, unknown>;
  paths?: Record<string, unknown>;
  mcps?: McpState;
  [key: string]: unknown;
}

export interface Session {
  id: number;
  name: string;
  state: SessionState;
  created_at?: string;
  updated_at?: string;
  last_used_at?: string;
  [key: string]: unknown;
}

export interface RuntimeStatePatch {
  session_id?: number;
  chat_id?: number;
  llm_model_name?: string;
  voice_model?: string;
  system_message?: string;
  developer_instructions?: string;
  mcps?: McpState;
  [key: string]: unknown;
}

export interface PrepareConversationInput extends RuntimeStatePatch {
  createChat?: boolean;
}

export type TurnEvent = AssistantStream | Assistant | ServerMessage;
export type TurnStatus = "completed" | "cancelled" | "error";

export interface TurnResult {
  clientRequestId: string;
  status: TurnStatus;
  text: string;
  code?: string;
  error?: string;
  terminal: AssistantComplete;
}

export interface TurnHandle {
  readonly clientRequestId: string;
  readonly events: AsyncIterable<TurnEvent>;
  readonly completed: Promise<TurnResult>;
  cancel(): Promise<void>;
}

export interface RunTurnInput {
  text: string;
  clientRequestId?: string;
  timeoutMs?: number;
  cancelTimeoutMs?: number;
  signal?: AbortSignal;
}

export interface RunConversationTurnInput extends RunTurnInput {
  setup: PrepareConversationInput;
}

export interface MageClientOptions {
  socket: BackendSocket;
  httpBaseUrl?: string;
  token?: string | null;
  fetchImpl?: typeof fetch;
  coordinator?: MutationCoordinator;
  observe?: (event: MageClientObservation) => void;
}

export interface MageClientConnectOptions
  extends Omit<MageClientOptions, "socket"> {
  wsUrl: string;
}

export interface MageClientObservation {
  event:
    | "connection_state"
    | "operation_started"
    | "operation_completed"
    | "mutation_acquired"
    | "setup_completed"
    | "first_assistant_token"
    | "mcp_warning"
    | "turn_submitted"
    | "turn_completed"
    | "turn_failed";
  timestamp: number;
  requestId?: string;
  status?: string;
  queueWaitMs?: number;
  durationMs?: number;
  connectionState?: string;
  operation?: string;
  warning?: string;
}

const sharedCoordinators = new Map<string, MutationCoordinator>();

class AsyncEventQueue<T> implements AsyncIterable<T> {
  private readonly values: T[] = [];
  private readonly waiters: Array<{
    resolve: (result: IteratorResult<T>) => void;
  }> = [];
  private ended = false;

  push(value: T): void {
    if (this.ended) return;
    const waiter = this.waiters.shift();
    if (waiter) waiter.resolve({ done: false, value });
    else this.values.push(value);
  }

  end(): void {
    if (this.ended) return;
    this.ended = true;
    for (const waiter of this.waiters.splice(0)) {
      waiter.resolve({ done: true, value: undefined });
    }
  }

  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        const value = this.values.shift();
        if (value !== undefined) {
          return Promise.resolve({ done: false, value });
        }
        if (this.ended) {
          return Promise.resolve({ done: true, value: undefined });
        }
        return new Promise((resolve) => this.waiters.push({ resolve }));
      },
    };
  }
}

export class MageClient {
  readonly socket: BackendSocket;
  private readonly httpBaseUrl?: string;
  private readonly token?: string | null;
  private readonly fetchImpl: typeof fetch;
  private readonly coordinator: MutationCoordinator;
  private readonly observe?: (event: MageClientObservation) => void;
  private readonly unsubscribeConnectionState: () => void;

  constructor(options: MageClientOptions) {
    this.socket = options.socket;
    this.httpBaseUrl = options.httpBaseUrl;
    this.token = options.token;
    this.fetchImpl = options.fetchImpl || fetch;
    this.coordinator = options.coordinator || new SerialMutationCoordinator();
    this.observe = options.observe;
    this.unsubscribeConnectionState = this.socket.onStateChange((connectionState) => {
      this.emit({ event: "connection_state", connectionState });
    });
  }

  static async connect(options: MageClientConnectOptions): Promise<MageClient> {
    try {
      const socket = await BackendSocket.connect(options.wsUrl, options.token);
      const coordinator =
        options.coordinator || coordinatorForBackend(options.wsUrl);
      return new MageClient({ ...options, coordinator, socket });
    } catch (cause) {
      throw new MageConnectionError("Could not connect to Mage backend", {
        cause,
        retryable: true,
      });
    }
  }

  close(): void {
    this.unsubscribeConnectionState();
    this.socket.close();
  }

  async listSessions(): Promise<Session[]> {
    const payload = await this.http<{ sessions?: Session[] }>("/api/sessions");
    return payload.sessions || [];
  }

  async getSession(sessionId: number): Promise<Session> {
    validatePositiveInteger(sessionId, "sessionId");
    const sessions = await this.listSessions();
    const session = sessions.find((candidate) => candidate.id === sessionId);
    if (!session) {
      throw new MageValidationError(`Mage session ${sessionId} was not found`, {
        code: "session_not_found",
      });
    }
    return session;
  }

  async createSession(input: {
    name?: string;
    state?: Partial<SessionState>;
  } = {}): Promise<Session> {
    return this.withMutation(async () => {
      const payload = await this.http<{ session?: Session }>("/api/sessions", {
        method: "POST",
        body: JSON.stringify({
          name: input.name || "headless",
          ...(input.state ? { state: input.state } : {}),
        }),
      });
      if (!payload.session) {
        throw new MageProtocolError("Session creation response omitted session");
      }
      if (input.state && !containsPartial(payload.session.state, input.state)) {
        throw new MageProtocolError(
          "Mage did not persist the requested partial session state; v0.12.0 or newer is required"
        );
      }
      return payload.session;
    });
  }

  async updateSession(
    sessionId: number,
    patch: { name?: string; state?: Partial<SessionState> }
  ): Promise<Session> {
    validatePositiveInteger(sessionId, "sessionId");
    return this.withMutation(async () => {
      const payload = await this.http<{ session?: Session }>(
        `/api/sessions/${sessionId}`,
        { method: "PATCH", body: JSON.stringify(patch) }
      );
      if (!payload.session) {
        throw new MageProtocolError("Session update response omitted session");
      }
      if (patch.state && !containsPartial(payload.session.state, patch.state)) {
        throw new MageProtocolError(
          "Mage did not persist the requested partial session update; v0.12.0 or newer is required"
        );
      }
      return payload.session;
    });
  }

  async writeRuntimeState(
    state: RuntimeStatePatch,
    timeoutMs = 30_000
  ): Promise<RuntimeStateWriteResult> {
    return this.withMutation(() =>
      this.writeRuntimeStateLocked(state, timeoutMs)
    );
  }

  private async writeRuntimeStateLocked(
    state: RuntimeStatePatch,
    timeoutMs = 30_000
  ): Promise<RuntimeStateWriteResult> {
    const requestId = randomUUID();
    const startedAt = Date.now();
    this.emit({ event: "operation_started", operation: "write_runtime_state", requestId });
    const result = await this.socket.requestById<RuntimeStateWriteResult>(
      { type: "write_runtime_state", request_id: requestId, state },
      "runtime_state_write_result",
      timeoutMs
    );
    this.assertOperationSucceeded(result, "Runtime state write", requestId);
    if (result.warnings?.length) {
      for (const warning of result.warnings) {
        this.emit({ event: "mcp_warning", requestId, warning });
      }
      throw new MagePartialFailureError(result.warnings.join("; "), {
        requestId,
        code: result.code || "runtime_state_partial_failure",
      });
    }
    this.emit({
      event: "operation_completed",
      operation: "write_runtime_state",
      requestId,
      durationMs: Date.now() - startedAt,
    });
    return result;
  }

  async createChat(timeoutMs = 30_000): Promise<NewChatResult> {
    return this.withMutation(() => this.createChatLocked(timeoutMs));
  }

  private async createChatLocked(timeoutMs = 30_000): Promise<NewChatResult> {
    const requestId = randomUUID();
    const startedAt = Date.now();
    this.emit({ event: "operation_started", operation: "new_chat", requestId });
    const result = await this.socket.requestById<NewChatResult>(
      { type: "new_chat", request_id: requestId },
      "new_chat_result",
      timeoutMs
    );
    this.assertOperationSucceeded(result, "Chat creation", requestId);
    if (!Number.isInteger(result.chat_id)) {
      throw new MageProtocolError("Chat creation response omitted chat_id", {
        requestId,
      });
    }
    this.emit({
      event: "operation_completed",
      operation: "new_chat",
      requestId,
      durationMs: Date.now() - startedAt,
    });
    return result;
  }

  async switchChat(chatId: number, timeoutMs = 30_000): Promise<ChatSwitchResult> {
    return this.withMutation(() => this.switchChatLocked(chatId, timeoutMs));
  }

  private async switchChatLocked(
    chatId: number,
    timeoutMs = 30_000
  ): Promise<ChatSwitchResult> {
    validatePositiveInteger(chatId, "chatId");
    const requestId = randomUUID();
    const startedAt = Date.now();
    this.emit({ event: "operation_started", operation: "set_chat", requestId });
    const result = await this.socket.requestById<ChatSwitchResult>(
      { type: "set_chat", chat_id: chatId, request_id: requestId },
      "chat_switch_result",
      timeoutMs
    );
    this.assertOperationSucceeded(result, "Chat switch", requestId);
    this.emit({
      event: "operation_completed",
      operation: "set_chat",
      requestId,
      durationMs: Date.now() - startedAt,
    });
    return result;
  }

  async prepareConversation(
    input: PrepareConversationInput,
    signal?: AbortSignal
  ): Promise<{ sessionId: number; chatId: number }> {
    const lease = await this.coordinator.acquire(signal);
    const startedAt = Date.now();
    try {
      const result = await this.prepareConversationLocked(input);
      this.emit({ event: "setup_completed", durationMs: Date.now() - startedAt });
      return result;
    } finally {
      lease.release();
    }
  }

  runTurn(input: RunTurnInput): TurnHandle {
    return this.startTurn(input, async (signal) =>
      this.coordinator.acquire(signal)
    );
  }

  runConversationTurn(input: RunConversationTurnInput): TurnHandle {
    return this.startTurn(input, async (signal) => {
      const lease = await this.coordinator.acquire(signal);
      try {
        const startedAt = Date.now();
        await this.prepareConversationLocked(input.setup);
        this.emit({ event: "setup_completed", durationMs: Date.now() - startedAt });
        return lease;
      } catch (error) {
        lease.release();
        throw error;
      }
    });
  }

  private async prepareConversationLocked(
    input: PrepareConversationInput
  ): Promise<{ sessionId: number; chatId: number }> {
    if (!Number.isInteger(input.session_id)) {
      throw new MageValidationError("prepareConversation requires session_id");
    }

    const runtimeState: RuntimeStatePatch = { ...input };
    delete (runtimeState as PrepareConversationInput).createChat;
    await this.writeRuntimeStateLocked(runtimeState);

    let chatId = input.chat_id;
    if (input.createChat) {
      const created = await this.createChatLocked();
      chatId = created.chat_id;
    } else if (chatId !== undefined) {
      await this.switchChatLocked(chatId);
    }

    if (!Number.isInteger(chatId)) {
      throw new MageSetupError("Conversation setup did not confirm a chat_id", {
        code: "chat_not_confirmed",
      });
    }
    return { sessionId: input.session_id!, chatId: chatId! };
  }

  private startTurn(
    input: RunTurnInput,
    acquireLease: (signal: AbortSignal) => Promise<MutationLease>
  ): TurnHandle {
    const text = input.text?.trim();
    if (!text) throw new MageValidationError("Assistant turn text is required");
    const clientRequestId = input.clientRequestId || randomUUID();
    validateIdentifier(clientRequestId, "clientRequestId");

    const queue = new AsyncEventQueue<TurnEvent>();
    const abortController = new AbortController();
    let active = false;
    let terminal = false;
    let cancelRequested = false;
    let rejectTerminal: ((error: Error) => void) | undefined;
    let cancellationTimeout: ReturnType<typeof setTimeout> | undefined;

    const externalAbort = () => void cancel();

    const completed = (async (): Promise<TurnResult> => {
      let lease: MutationLease | undefined;
      const unsubscribers: Array<() => void> = [];
      let timeout: ReturnType<typeof setTimeout> | undefined;
      let buffered = "";
      let firstTokenObserved = false;
      const startedAt = Date.now();

      try {
        const queueStartedAt = Date.now();
        lease = await acquireLease(abortController.signal);
        this.emit({
          event: "mutation_acquired",
          requestId: clientRequestId,
          queueWaitMs: Date.now() - queueStartedAt,
        });
        if (cancelRequested || abortController.signal.aborted) {
          return cancelledBeforeSend(clientRequestId);
        }
        active = true;

        const terminalPromise = new Promise<AssistantComplete>((resolve, reject) => {
          rejectTerminal = reject;
          const onCorrelated = (message: ServerMessage) => {
            if (
              !("client_request_id" in message) ||
              message.client_request_id !== clientRequestId
            ) {
              return;
            }

            queue.push(message);
            if (message.type === "assistant_stream" && message.phase === "delta") {
              const delta = message.token || message.text || message.content || "";
              buffered += delta;
              if (delta && !firstTokenObserved) {
                firstTokenObserved = true;
                this.emit({
                  event: "first_assistant_token",
                  requestId: clientRequestId,
                  durationMs: Date.now() - startedAt,
                });
              }
            } else if (message.type === "assistant" && message.text) {
              buffered += message.text;
              if (!firstTokenObserved) {
                firstTokenObserved = true;
                this.emit({
                  event: "first_assistant_token",
                  requestId: clientRequestId,
                  durationMs: Date.now() - startedAt,
                });
              }
            } else if (message.type === "assistant_complete") {
              resolve(message);
            }
          };

          for (const type of [
            "assistant_stream",
            "assistant",
            "transcription",
            "assistant_complete",
          ]) {
            unsubscribers.push(this.socket.on(type, onCorrelated));
          }
          unsubscribers.push(
            this.socket.onStateChange((state) => {
              if (state !== "connected") {
                reject(
                  new MageConnectionLostError(
                    "Mage connection was lost before terminal completion; turn outcome is unknown",
                    { requestId: clientRequestId, retryable: false }
                  )
                );
              }
            })
          );

          timeout = setTimeout(() => {
            reject(
              new MageTimeoutError(
                `Assistant turn ${clientRequestId} timed out`,
                { requestId: clientRequestId, retryable: false }
              )
            );
          }, input.timeoutMs || 120_000);
        });

        this.socket.send({
          type: "text",
          text,
          client_request_id: clientRequestId,
        });
        this.emit({ event: "turn_submitted", requestId: clientRequestId });

        const completion = await terminalPromise;
        terminal = true;
        const status = (completion.status || "completed") as TurnStatus;
        const result = {
          clientRequestId,
          status,
          text: buffered,
          code: completion.code,
          error: completion.error,
          terminal: completion,
        };
        this.emit({
          event: "turn_completed",
          requestId: clientRequestId,
          status,
          durationMs: Date.now() - startedAt,
        });
        return result;
      } catch (error) {
        this.emit({
          event: "turn_failed",
          requestId: clientRequestId,
          durationMs: Date.now() - startedAt,
        });
        throw error;
      } finally {
        terminal = true;
        if (timeout) clearTimeout(timeout);
        if (cancellationTimeout) clearTimeout(cancellationTimeout);
        for (const unsubscribe of unsubscribers) unsubscribe();
        input.signal?.removeEventListener("abort", externalAbort);
        queue.end();
        lease?.release();
      }
    })();

    const cancel = async (): Promise<void> => {
      if (terminal || cancelRequested) return;
      cancelRequested = true;
      if (!active) {
        abortController.abort();
        return;
      }
      this.socket.send({ type: "control", action: "stop" });
      cancellationTimeout = setTimeout(() => {
        rejectTerminal?.(
          new MageTimeoutError(
            `Cancellation for assistant turn ${clientRequestId} timed out`,
            { requestId: clientRequestId, retryable: false }
          )
        );
      }, input.cancelTimeoutMs || 10_000);
    };

    if (input.signal?.aborted) void cancel();
    else input.signal?.addEventListener("abort", externalAbort, { once: true });

    return { clientRequestId, events: queue, completed, cancel };
  }

  private assertOperationSucceeded(
    result: { ok?: boolean; code?: string; error?: string },
    operation: string,
    requestId: string
  ): void {
    if (result.ok === true) return;
    throw new MageSetupError(result.error || `${operation} failed`, {
      requestId,
      code: result.code,
    });
  }

  private async withMutation<T>(operation: () => Promise<T>): Promise<T> {
    const startedAt = Date.now();
    const lease = await this.coordinator.acquire();
    this.emit({ event: "mutation_acquired", queueWaitMs: Date.now() - startedAt });
    try {
      return await operation();
    } finally {
      lease.release();
    }
  }

  private emit(event: Omit<MageClientObservation, "timestamp">): void {
    this.observe?.({ ...event, timestamp: Date.now() });
  }

  private async http<T>(path: string, init: RequestInit = {}): Promise<T> {
    if (!this.httpBaseUrl) {
      throw new MageConnectionError(
        "Mage HTTP base URL is required for session operations"
      );
    }
    const headers = new Headers(init.headers);
    headers.set("content-type", "application/json");
    if (this.token) headers.set("authorization", `Bearer ${this.token}`);

    let response: Response;
    try {
      response = await this.fetchImpl(
        `${this.httpBaseUrl.replace(/\/$/, "")}${path}`,
        { ...init, headers }
      );
    } catch (cause) {
      throw new MageConnectionError(`Mage request failed: ${path}`, {
        cause,
        retryable: true,
      });
    }

    let payload: unknown;
    try {
      payload = await response.json();
    } catch (cause) {
      throw new MageProtocolError(`Mage returned invalid JSON for ${path}`, {
        cause,
      });
    }
    if (!response.ok) {
      const body = payload as { error?: string; code?: string };
      throw new MageValidationError(
        body.error || `Mage request failed with HTTP ${response.status}`,
        { code: body.code }
      );
    }
    return payload as T;
  }
}

function validatePositiveInteger(value: number, field: string): void {
  if (!Number.isInteger(value) || value <= 0) {
    throw new MageValidationError(`${field} must be a positive integer`);
  }
}

function validateIdentifier(value: string, field: string): void {
  if (!value.trim() || Buffer.byteLength(value, "utf8") > 256) {
    throw new MageValidationError(
      `${field} must be a non-empty string of at most 256 UTF-8 bytes`
    );
  }
}

function cancelledBeforeSend(clientRequestId: string): TurnResult {
  return {
    clientRequestId,
    status: "cancelled",
    text: "",
    terminal: {
      type: "assistant_complete",
      client_request_id: clientRequestId,
      status: "cancelled",
    },
  };
}

function coordinatorForBackend(wsUrl: string): MutationCoordinator {
  const key = normalizedBackendKey(wsUrl);
  let coordinator = sharedCoordinators.get(key);
  if (!coordinator) {
    coordinator = new SerialMutationCoordinator();
    sharedCoordinators.set(key, coordinator);
  }
  return coordinator;
}

function normalizedBackendKey(wsUrl: string): string {
  try {
    const url = new URL(wsUrl);
    for (const name of ["token", "gateway_token", "ws_ticket"]) {
      url.searchParams.delete(name);
    }
    url.hash = "";
    return url.toString();
  } catch {
    return wsUrl;
  }
}

function containsPartial(actual: unknown, expected: unknown): boolean {
  if (Array.isArray(expected)) {
    return Array.isArray(actual) &&
      expected.length === actual.length &&
      expected.every((value, index) => containsPartial(actual[index], value));
  }
  if (expected && typeof expected === "object") {
    if (!actual || typeof actual !== "object") return false;
    return Object.entries(expected).every(([key, value]) =>
      containsPartial((actual as Record<string, unknown>)[key], value)
    );
  }
  return Object.is(actual, expected);
}
