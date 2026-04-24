/**
 * Full integration test: exercises the complete init flow
 * from connection through tool registration and execution,
 * including the confirmation auto-approval path.
 */
import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { BackendSocket } from "../src/websocket.js";
import { registerBackendTools } from "../src/tools.js";
import { MockBackend } from "./mock-backend.js";

describe("full integration flow", () => {
  let backend: MockBackend;

  beforeAll(async () => {
    backend = new MockBackend({
      requireConfirmation: true,
      onToolCall: (name, args) => {
        if (name === "confirmed_tool") return "tool executed after confirmation";
        return `${name}: ${JSON.stringify(args)}`;
      },
    });
    await backend.start();
  });

  afterAll(async () => {
    await backend.close();
  });

  it("connect -> register tools -> execute with confirmation -> get result", async () => {
    // 1. Connect (simulates what index.ts does after magelab connect --json)
    const socket = await BackendSocket.connect(backend.url);
    expect(socket.closed).toBe(false);

    // 2. Set up confirmation auto-approval (like index.ts does)
    socket.on("confirmation_request", (msg: any) => {
      socket.send({
        type: "confirmation_response",
        confirmation_id: msg.confirmation_id,
        confirmed: true,
        remember: false,
      });
    });

    // 3. Register tools
    const pi = { registerTool: (def: any) => tools.push(def) };
    const tools: any[] = [];
    const count = await registerBackendTools(pi, socket);

    expect(count).toBeGreaterThan(0);
    const runPython = tools.find((t) => t.name === "run_python");
    expect(runPython).toBeDefined();

    // 4. Execute a tool — this triggers confirmation flow:
    //    tool_call -> confirmation_request -> auto-approve -> tool_call_result
    const result = await runPython.execute(
      "integration-call",
      { code: "print('integration test')" },
      undefined,
      undefined,
      {}
    );

    expect(result.content[0].text).toContain("tool executed after confirmation");

    // 5. Clean shutdown
    socket.close();
    expect(socket.closed).toBe(true);
  });

  it("multiple tools execute independently with confirmation", async () => {
    const socket = await BackendSocket.connect(backend.url);

    socket.on("confirmation_request", (msg: any) => {
      socket.send({
        type: "confirmation_response",
        confirmation_id: msg.confirmation_id,
        confirmed: true,
        remember: false,
      });
    });

    const pi = { registerTool: (def: any) => tools.push(def) };
    const tools: any[] = [];
    await registerBackendTools(pi, socket);

    const runPython = tools.find((t) => t.name === "run_python");
    const searchWeb = tools.find((t) => t.name === "search_web");

    // Execute two tools sequentially (confirmation flow is serial per tool)
    const r1 = await runPython.execute("call-a", { code: "1+1" }, undefined, undefined, {});
    const r2 = await searchWeb.execute("call-b", { query: "test" }, undefined, undefined, {});

    expect(r1.content[0].text).toContain("confirmation");
    expect(r2.content[0].text).toContain("confirmation");

    socket.close();
  });
});

describe("error scenarios", () => {
  it("tool execution error propagates cleanly", async () => {
    const backend = new MockBackend({
      onToolCall: () => { throw new Error("ImportError: no module named 'foo'"); },
    });
    await backend.start();

    const socket = await BackendSocket.connect(backend.url);
    const tools: any[] = [];
    await registerBackendTools({ registerTool: (d: any) => tools.push(d) }, socket);

    const python = tools.find((t) => t.name === "run_python");
    const result = await python.execute("err-1", { code: "import foo" }, undefined, undefined, {});

    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("ImportError");

    socket.close();
    await backend.close();
  });

  it("slow tool respects timeout", async () => {
    const backend = new MockBackend({ toolDelay: 500 });
    await backend.start();

    const socket = await BackendSocket.connect(backend.url);

    // Direct callTool with short timeout
    await expect(
      socket.callTool("run_python", { code: "1" }, 100)
    ).rejects.toThrow("timed out");

    socket.close();
    await backend.close();
  });
});
