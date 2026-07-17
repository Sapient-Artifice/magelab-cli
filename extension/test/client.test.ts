import { WebSocket } from "ws";
import { afterEach, describe, expect, it } from "vitest";
import {
  MageClient,
  MagePartialFailureError,
  MageSetupError,
  SerialMutationCoordinator,
} from "../src/client/index.js";
import { BackendSocket } from "../src/websocket.js";
import { MockBackend } from "./mock-backend.js";

const cleanups: Array<() => void | Promise<void>> = [];

afterEach(async () => {
  for (const cleanup of cleanups.reverse()) await cleanup();
  cleanups.length = 0;
});

async function connectedClient(
  backend: MockBackend,
  options: ConstructorParameters<typeof MageClient>[0] extends infer T
    ? Partial<T>
    : never = {}
): Promise<MageClient> {
  await backend.start();
  cleanups.push(() => backend.close());
  const socket = await BackendSocket.connect(backend.url);
  cleanups.push(() => socket.close());
  return new MageClient({ socket, ...options });
}

describe("MageClient correlation and setup", () => {
  it("correlates same-type responses by request_id when they arrive out of order", async () => {
    const waiting: Array<{ ws: WebSocket; data: any }> = [];
    const backend = new MockBackend({
      onMessage(ws, data) {
        if (data.type !== "new_chat") return false;
        waiting.push({ ws, data });
        if (waiting.length === 2) {
          for (const item of waiting.reverse()) {
            item.ws.send(JSON.stringify({
              type: "new_chat_result",
              request_id: item.data.request_id,
              ok: true,
              chat_id: item.data.request_id === "first" ? 1 : 2,
            }));
          }
        }
        return true;
      },
    });
    const client = await connectedClient(backend);

    const first = client.socket.requestById<any>(
      { type: "new_chat", request_id: "first" },
      "new_chat_result"
    );
    const second = client.socket.requestById<any>(
      { type: "new_chat", request_id: "second" },
      "new_chat_result"
    );

    await expect(first).resolves.toMatchObject({ request_id: "first", chat_id: 1 });
    await expect(second).resolves.toMatchObject({ request_id: "second", chat_id: 2 });
  });

  it("runs runtime setup before chat creation and sends no prompt", async () => {
    const backend = new MockBackend();
    const client = await connectedClient(backend);

    const result = await client.prepareConversation({
      session_id: 42,
      llm_model_name: "model-a",
      mcps: { enabled_servers: ["pipedrive"] },
      createChat: true,
    });

    expect(result).toEqual({ sessionId: 42, chatId: 100 });
    expect(backend.receivedMessages.map((message) => message.type)).toEqual([
      "write_runtime_state",
      "new_chat",
    ]);
  });

  it("does not create a chat after runtime setup failure", async () => {
    const backend = new MockBackend({
      onMessage(ws, data) {
        if (data.type !== "write_runtime_state") return false;
        ws.send(JSON.stringify({
          type: "runtime_state_write_result",
          request_id: data.request_id,
          ok: false,
          code: "session_not_found",
          error: "missing session",
        }));
        return true;
      },
    });
    const client = await connectedClient(backend);

    await expect(
      client.prepareConversation({ session_id: 404, createChat: true })
    ).rejects.toBeInstanceOf(MageSetupError);
    expect(backend.receivedMessages.map((message) => message.type)).toEqual([
      "write_runtime_state",
    ]);
  });

  it("does not send a prompt after chat creation failure", async () => {
    const backend = new MockBackend({
      onMessage(ws, data) {
        if (data.type !== "new_chat") return false;
        ws.send(JSON.stringify({
          type: "new_chat_result",
          request_id: data.request_id,
          ok: false,
          code: "chat_create_failed",
          error: "database unavailable",
        }));
        return true;
      },
    });
    const client = await connectedClient(backend);
    const handle = client.runConversationTurn({
      text: "must not send",
      setup: { session_id: 1, createChat: true },
    });

    await expect(handle.completed).rejects.toBeInstanceOf(MageSetupError);
    expect(backend.receivedMessages.map((message) => message.type)).toEqual([
      "write_runtime_state",
      "new_chat",
    ]);
  });

  it("surfaces MCP reconciliation warnings as a partial failure", async () => {
    const backend = new MockBackend({
      onMessage(ws, data) {
        if (data.type !== "write_runtime_state") return false;
        ws.send(JSON.stringify({
          type: "runtime_state_write_result",
          request_id: data.request_id,
          ok: true,
          warnings: ["MCP reconciliation failed"],
        }));
        return true;
      },
    });
    const client = await connectedClient(backend);

    await expect(
      client.writeRuntimeState({
        session_id: 1,
        mcps: { enabled_servers: ["pipedrive"] },
      })
    ).rejects.toBeInstanceOf(MagePartialFailureError);
  });
});

describe("MageClient assistant turns", () => {
  it("waits for assistant_complete across stream end and a follow-up stream", async () => {
    const backend = new MockBackend({
      onText(ws, data) {
        const id = data.client_request_id;
        for (const message of [
          { type: "assistant_stream", phase: "delta", token: "first ", client_request_id: id },
          { type: "assistant_stream", phase: "end", client_request_id: id },
          { type: "tool_result", function_name: "lookup", result: "done" },
          { type: "assistant_stream", phase: "delta", token: "second", client_request_id: id },
          { type: "assistant_stream", phase: "end", client_request_id: id },
          { type: "assistant_complete", client_request_id: id, status: "completed" },
        ]) {
          ws.send(JSON.stringify(message));
        }
      },
    });
    const client = await connectedClient(backend);
    const handle = client.runTurn({ text: "hello", clientRequestId: "turn-1" });

    await expect(handle.completed).resolves.toMatchObject({
      clientRequestId: "turn-1",
      status: "completed",
      text: "first second",
    });
    expect(client.socket.listenerCount("assistant_complete")).toBe(0);
    expect(client.socket.pendingCount()).toBe(0);
  });

  it("waits for terminal completion after a non-streamed assistant message", async () => {
    let complete: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => (complete = resolve));
    const backend = new MockBackend({
      async onText(ws, data) {
        ws.send(JSON.stringify({
          type: "assistant",
          text: "whole response",
          client_request_id: data.client_request_id,
        }));
        await gate;
        ws.send(JSON.stringify({
          type: "assistant_complete",
          client_request_id: data.client_request_id,
          status: "completed",
        }));
      },
    });
    const client = await connectedClient(backend);
    const handle = client.runTurn({ text: "hello" });
    let settled = false;
    void handle.completed.then(() => (settled = true));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(settled).toBe(false);
    complete!();
    await expect(handle.completed).resolves.toMatchObject({ text: "whole response" });
  });

  it("ignores assistant events carrying another turn id", async () => {
    const backend = new MockBackend({
      onText(ws, data) {
        ws.send(JSON.stringify({
          type: "assistant_stream",
          phase: "delta",
          token: "wrong",
          client_request_id: "another-turn",
        }));
        ws.send(JSON.stringify({
          type: "assistant_stream",
          phase: "delta",
          token: "right",
          client_request_id: data.client_request_id,
        }));
        ws.send(JSON.stringify({
          type: "assistant_complete",
          client_request_id: data.client_request_id,
          status: "completed",
        }));
      },
    });
    const client = await connectedClient(backend);
    const result = await client.runTurn({ text: "hello" }).completed;
    expect(result.text).toBe("right");
  });

  it("preserves assistant error terminal status and safe error fields", async () => {
    const backend = new MockBackend({
      onText(ws, data) {
        ws.send(JSON.stringify({
          type: "assistant_complete",
          client_request_id: data.client_request_id,
          status: "error",
          code: "model_failed",
          error: "The model request failed",
        }));
      },
    });
    const client = await connectedClient(backend);
    await expect(client.runTurn({ text: "hello" }).completed).resolves.toMatchObject({
      status: "error",
      code: "model_failed",
      error: "The model request failed",
    });
  });

  it("rejects immediately when the connection is lost without replaying the prompt", async () => {
    const backend = new MockBackend({
      onText(ws) {
        ws.close();
      },
    });
    const client = await connectedClient(backend);
    const handle = client.runTurn({ text: "side effect", timeoutMs: 30_000 });

    await expect(handle.completed).rejects.toThrow("outcome is unknown");
    expect(
      backend.receivedMessages.filter((message) => message.type === "text")
    ).toHaveLength(1);
    expect(client.socket.listenerCount("assistant_complete")).toBe(0);
  });

  it("cleans up listeners and pending state after a turn timeout", async () => {
    const backend = new MockBackend({ onText() {} });
    const client = await connectedClient(backend);
    const handle = client.runTurn({ text: "never completes", timeoutMs: 30 });

    await expect(handle.completed).rejects.toThrow("timed out");
    expect(client.socket.listenerCount("assistant_stream")).toBe(0);
    expect(client.socket.listenerCount("assistant_complete")).toBe(0);
    expect(client.socket.pendingCount()).toBe(0);
  });

  it("does not accumulate scoped listeners across sequential turns", async () => {
    const backend = new MockBackend();
    const client = await connectedClient(backend);
    for (let index = 0; index < 5; index++) {
      await client.runTurn({ text: `turn ${index}` }).completed;
      expect(client.socket.listenerCount("assistant_complete")).toBe(0);
    }
  });

  it("cancels the active serialized turn and waits for terminal cancellation", async () => {
    const backend = new MockBackend({ onText() {} });
    const client = await connectedClient(backend);
    const handle = client.runTurn({ text: "long task" });

    while (!backend.receivedMessages.some((message) => message.type === "text")) {
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    await handle.cancel();

    await expect(handle.completed).resolves.toMatchObject({ status: "cancelled" });
    expect(backend.receivedMessages.at(-1)).toMatchObject({
      type: "control",
      action: "stop",
    });
  });

  it("cleans up when cancellation is not acknowledged", async () => {
    const backend = new MockBackend({
      onText() {},
      onMessage(_ws, data) {
        return data.type === "control";
      },
    });
    const client = await connectedClient(backend);
    const handle = client.runTurn({
      text: "long task",
      timeoutMs: 30_000,
      cancelTimeoutMs: 30,
    });
    while (!backend.receivedMessages.some((message) => message.type === "text")) {
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    await handle.cancel();

    await expect(handle.completed).rejects.toThrow("Cancellation");
    expect(client.socket.listenerCount("assistant_complete")).toBe(0);
  });

  it("serializes complete turns across clients sharing a coordinator", async () => {
    const pending: Array<{ ws: WebSocket; id: string }> = [];
    const backend = new MockBackend({
      onText(ws, data) {
        pending.push({ ws, id: data.client_request_id });
      },
    });
    const coordinator = new SerialMutationCoordinator();
    const client = await connectedClient(backend, { coordinator });
    const secondSocket = await BackendSocket.connect(backend.url);
    cleanups.push(() => secondSocket.close());
    const secondClient = new MageClient({ socket: secondSocket, coordinator });

    const first = client.runTurn({ text: "first", clientRequestId: "first" });
    const second = secondClient.runTurn({ text: "second", clientRequestId: "second" });

    while (pending.length < 1) await new Promise((resolve) => setTimeout(resolve, 5));
    expect(pending).toHaveLength(1);
    pending[0].ws.send(JSON.stringify({
      type: "assistant_complete",
      client_request_id: pending[0].id,
      status: "completed",
    }));
    await first.completed;

    while (pending.length < 2) await new Promise((resolve) => setTimeout(resolve, 5));
    pending[1].ws.send(JSON.stringify({
      type: "assistant_complete",
      client_request_id: pending[1].id,
      status: "completed",
    }));
    await second.completed;
    expect(pending.map((item) => item.id)).toEqual(["first", "second"]);
  });

  it("shares serialization automatically for connections to the same backend", async () => {
    const pending: Array<{ ws: WebSocket; id: string }> = [];
    const backend = new MockBackend({
      onText(ws, data) {
        pending.push({ ws, id: data.client_request_id });
      },
    });
    await backend.start();
    cleanups.push(() => backend.close());
    const firstClient = await MageClient.connect({ wsUrl: backend.url });
    const secondClient = await MageClient.connect({ wsUrl: backend.url });
    cleanups.push(() => firstClient.close());
    cleanups.push(() => secondClient.close());

    const first = firstClient.runTurn({ text: "first", clientRequestId: "auto-first" });
    const second = secondClient.runTurn({ text: "second", clientRequestId: "auto-second" });
    while (pending.length < 1) await new Promise((resolve) => setTimeout(resolve, 5));
    expect(pending).toHaveLength(1);
    pending[0].ws.send(JSON.stringify({
      type: "assistant_complete",
      client_request_id: pending[0].id,
      status: "completed",
    }));
    await first.completed;
    while (pending.length < 2) await new Promise((resolve) => setTimeout(resolve, 5));
    pending[1].ws.send(JSON.stringify({
      type: "assistant_complete",
      client_request_id: pending[1].id,
      status: "completed",
    }));
    await second.completed;
    expect(pending.map((item) => item.id)).toEqual(["auto-first", "auto-second"]);
  });

  it("emits metadata-only observability events", async () => {
    const backend = new MockBackend();
    const observations: any[] = [];
    const client = await connectedClient(backend, {
      observe: (event) => observations.push(event),
    });
    await client.runTurn({ text: "secret prompt", clientRequestId: "observed" }).completed;

    expect(observations.map((event) => event.event)).toEqual([
      "mutation_acquired",
      "turn_submitted",
      "first_assistant_token",
      "turn_completed",
    ]);
    expect(JSON.stringify(observations)).not.toContain("secret prompt");
  });
});

describe("MageClient session HTTP API", () => {
  it("creates a session with canonical MCP state", async () => {
    const backend = new MockBackend();
    const requests: Array<{ url: string; init?: RequestInit }> = [];
    const fetchImpl = async (url: string | URL | Request, init?: RequestInit) => {
      requests.push({ url: String(url), init });
      return new Response(JSON.stringify({
        success: true,
        session: {
          id: 7,
          name: "CRM",
          state: { mcps: { enabled_servers: ["pipedrive"] } },
        },
      }), { status: 200, headers: { "content-type": "application/json" } });
    };
    const client = await connectedClient(backend, {
      httpBaseUrl: "http://mage.test",
      fetchImpl: fetchImpl as typeof fetch,
    });

    const session = await client.createSession({
      name: "CRM",
      state: { mcps: { enabled_servers: ["pipedrive"] } },
    });

    expect(session.id).toBe(7);
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      name: "CRM",
      state: { mcps: { enabled_servers: ["pipedrive"] } },
    });
  });

  it("surfaces invalid MCP server validation errors", async () => {
    const backend = new MockBackend();
    const fetchImpl = async () =>
      new Response(JSON.stringify({
        code: "unknown_mcp_server",
        error: "Unknown MCP server: missing",
      }), { status: 400, headers: { "content-type": "application/json" } });
    const client = await connectedClient(backend, {
      httpBaseUrl: "http://mage.test",
      fetchImpl: fetchImpl as typeof fetch,
    });

    await expect(
      client.createSession({
        state: { mcps: { enabled_servers: ["missing"] } },
      })
    ).rejects.toMatchObject({
      name: "MageValidationError",
      code: "unknown_mcp_server",
    });
  });

  it("sends partial session updates without replacing unrelated state", async () => {
    const backend = new MockBackend();
    let body: any;
    const fetchImpl = async (_url: string | URL | Request, init?: RequestInit) => {
      body = JSON.parse(String(init?.body));
      return new Response(JSON.stringify({
        success: true,
        session: { id: 7, name: "CRM", state: body.state },
      }), { status: 200, headers: { "content-type": "application/json" } });
    };
    const client = await connectedClient(backend, {
      httpBaseUrl: "http://mage.test",
      fetchImpl: fetchImpl as typeof fetch,
    });

    await client.updateSession(7, {
      state: { mcps: { enabled_servers: ["hubspot"] } },
    });
    expect(body).toEqual({
      state: { mcps: { enabled_servers: ["hubspot"] } },
    });
  });
});
