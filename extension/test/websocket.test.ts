import { describe, it, expect, beforeAll, afterAll, afterEach } from "vitest";
import { BackendSocket } from "../src/websocket.js";
import { MockBackend } from "./mock-backend.js";

let backend: MockBackend;
let cleanup: (() => void)[] = [];

beforeAll(async () => {
  backend = new MockBackend();
  await backend.start();
});

afterAll(async () => {
  await backend.close();
});

afterEach(() => {
  for (const fn of cleanup) fn();
  cleanup = [];
});

describe("BackendSocket", () => {
  it("connects to backend", async () => {
    const socket = await BackendSocket.connect(backend.url);
    expect(socket.closed).toBe(false);
    socket.close();
  });

  it("fetches tool list via get_tools", async () => {
    const socket = await BackendSocket.connect(backend.url);

    const result = await socket.requestByType(
      { type: "get_tools" },
      "tools_list"
    );

    expect(result.type).toBe("tools_list");
    const tools = (result as any).tools;
    expect(tools.length).toBeGreaterThan(0);

    const names = tools.map((t: any) => t.function.name);
    expect(names).toContain("run_python");
    expect(names).toContain("search_web");
    expect(names).toContain("run_bash");

    socket.close();
  });

  it("executes a tool call and correlates by call_id", async () => {
    const socket = await BackendSocket.connect(backend.url);

    const result = await socket.callTool("run_python", { code: "print(42)" });

    expect(result.success).toBe(true);
    expect(result.result).toContain("run_python");
    expect(result.result).toContain("print(42)");

    socket.close();
  });

  it("handles multiple concurrent tool calls", async () => {
    const socket = await BackendSocket.connect(backend.url);

    const [r1, r2, r3] = await Promise.all([
      socket.callTool("run_python", { code: "1" }),
      socket.callTool("search_web", { query: "test" }),
      socket.callTool("calculate", { expression: "2+2" }),
    ]);

    expect(r1.success).toBe(true);
    expect(r1.result).toContain("run_python");
    expect(r2.success).toBe(true);
    expect(r2.result).toContain("search_web");
    expect(r3.success).toBe(true);
    expect(r3.result).toContain("calculate");

    socket.close();
  });

  it("times out if backend never responds", async () => {
    // Create a server that ignores tool_call messages
    const silentBackend = new MockBackend({
      onToolCall: () => new Promise(() => {}), // Never resolves
    });
    await silentBackend.start();

    const socket = await BackendSocket.connect(silentBackend.url);

    await expect(
      socket.callTool("run_python", { code: "1" }, 200)
    ).rejects.toThrow("timed out");

    socket.close();
    await silentBackend.close();
  });

  it("rejects pending calls on close", async () => {
    const silentBackend = new MockBackend({
      onToolCall: () => new Promise(() => {}),
    });
    await silentBackend.start();

    const socket = await BackendSocket.connect(silentBackend.url);
    const promise = socket.callTool("run_python", { code: "1" }, 30_000);

    socket.close();

    await expect(promise).rejects.toThrow("WebSocket closed");
    await silentBackend.close();
  });

  it("rejects connection to invalid url", async () => {
    await expect(
      BackendSocket.connect("ws://127.0.0.1:1")
    ).rejects.toThrow();
  });

  it("receives persistent handler messages", async () => {
    const confirmBackend = new MockBackend({ requireConfirmation: true });
    await confirmBackend.start();

    const socket = await BackendSocket.connect(confirmBackend.url);
    const received: any[] = [];

    socket.on("confirmation_request", (msg) => {
      received.push(msg);
      // Auto-approve so the tool call completes
      socket.send({
        type: "confirmation_response",
        confirmation_id: (msg as any).confirmation_id,
        confirmed: true,
        remember: false,
      });
    });

    const result = await socket.callTool("run_bash", { command: "ls" });

    expect(received).toHaveLength(1);
    expect(received[0].type).toBe("confirmation_request");
    expect(received[0].function_name).toBe("run_bash");
    expect(result.success).toBe(true);

    socket.close();
    await confirmBackend.close();
  });

  it("reports state changes via onStateChange", async () => {
    const socket = await BackendSocket.connect(backend.url);
    cleanup.push(() => socket.close());

    const states: string[] = [];
    socket.onStateChange((s) => states.push(s));

    expect(socket.state).toBe("connected");

    // Intentional close
    socket.close();
    expect(socket.state).toBe("disconnected");
  });

  it("appends token as query parameter for relay URLs", async () => {
    // Create a backend that records the connection URL
    const tokenBackend = new MockBackend();
    await tokenBackend.start();

    // Connect with a token — the URL should get the token appended
    const baseUrl = `${tokenBackend.url}?ws_ticket=abc123`;
    const socket = await BackendSocket.connect(baseUrl, "my_jwt_token");

    // Verify connection succeeded (token was transmitted, not dropped)
    expect(socket.closed).toBe(false);
    socket.close();
    await tokenBackend.close();
  });

  it("explicit close does not fire reconnecting state", async () => {
    const socket = await BackendSocket.connect(backend.url);

    const states: string[] = [];
    socket.onStateChange((s) => states.push(s));

    socket.close();

    // Wait for any async close events to propagate
    await new Promise((r) => setTimeout(r, 100));

    // Should NOT contain "reconnecting" — only "disconnected" is acceptable
    expect(states).not.toContain("reconnecting");
    expect(socket.state).toBe("disconnected");
  });

  it("rejects concurrent uncorrelated requests of the same type", async () => {
    const socket = await BackendSocket.connect(backend.url);

    const first = socket.requestByType(
      { type: "get_tools" },
      "tools_list"
    );
    const second = socket.requestByType(
      { type: "get_tools" },
      "tools_list"
    );

    await expect(second).rejects.toThrow("Only one uncorrelated");
    await expect(first).resolves.toMatchObject({ type: "tools_list" });

    socket.close();
  });

  it("rejects an uncorrelated response instead of waiting for timeout", async () => {
    const legacyBackend = new MockBackend({
      onMessage(ws, data) {
        if (data.type !== "new_chat") return false;
        ws.send(JSON.stringify({ type: "new_chat_result", ok: true, chat_id: 1 }));
        return true;
      },
    });
    await legacyBackend.start();
    const socket = await BackendSocket.connect(legacyBackend.url);

    await expect(
      socket.requestById({ type: "new_chat" }, "new_chat_result", 30_000)
    ).rejects.toThrow("v0.12.0 or newer");
    expect(socket.pendingCount()).toBe(0);

    socket.close();
    await legacyBackend.close();
  });

  it("validates caller-supplied request identifiers", async () => {
    const socket = await BackendSocket.connect(backend.url);
    await expect(
      socket.requestById(
        { type: "new_chat", request_id: "x".repeat(257) },
        "new_chat_result"
      )
    ).rejects.toThrow("at most 256");
    expect(socket.pendingCount()).toBe(0);
    socket.close();
  });

  it("does not let an unknown request_id resolve a pending operation", async () => {
    const correlationBackend = new MockBackend({
      onMessage(ws, data) {
        if (data.type !== "new_chat") return false;
        ws.send(JSON.stringify({
          type: "new_chat_result",
          request_id: "unknown",
          ok: true,
          chat_id: 1,
        }));
        ws.send(JSON.stringify({
          type: "new_chat_result",
          request_id: data.request_id,
          ok: true,
          chat_id: 2,
        }));
        return true;
      },
    });
    await correlationBackend.start();
    const socket = await BackendSocket.connect(correlationBackend.url);

    await expect(
      socket.requestById(
        { type: "new_chat", request_id: "expected" },
        "new_chat_result"
      )
    ).resolves.toMatchObject({ request_id: "expected", chat_id: 2 });

    socket.close();
    await correlationBackend.close();
  });
});
