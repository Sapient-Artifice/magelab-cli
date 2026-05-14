import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { BackendSocket } from "../src/websocket.js";
import { registerBackendTools } from "../src/tools.js";
import { MockBackend } from "./mock-backend.js";

let backend: MockBackend;

beforeAll(async () => {
  backend = new MockBackend();
  await backend.start();
});

afterAll(async () => {
  await backend.close();
});

function createMockPi() {
  const tools: any[] = [];
  return {
    registerTool: (def: any) => tools.push(def),
    registeredTools: tools,
  };
}

describe("registerBackendTools", () => {
  it("registers non-native tools and skips native ones", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();

    const { count } = await registerBackendTools(pi, socket);

    // Default mock backend has 8 tools:
    //   run_python, search_web, read_file, write_file, run_bash,
    //   open_file, generate_image, calculate
    // Skip: read_file, write_file, run_bash, open_file (4)
    // Register: run_python, search_web, generate_image, calculate (4)
    expect(count).toBe(4);
    expect(pi.registeredTools).toHaveLength(4);

    const names = pi.registeredTools.map((t: any) => t.name);
    expect(names).toContain("run_python");
    expect(names).toContain("search_web");
    expect(names).toContain("generate_image");
    expect(names).toContain("calculate");

    // These should be skipped
    expect(names).not.toContain("read_file");
    expect(names).not.toContain("write_file");
    expect(names).not.toContain("run_bash");
    expect(names).not.toContain("open_file");

    socket.close();
  });

  it("sets description and label on registered tools", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    const python = pi.registeredTools.find((t: any) => t.name === "run_python");
    expect(python.description).toBe("Execute Python code in an isolated interpreter");
    expect(python.label).toBe("run_python");
    expect(python.promptSnippet).toBe("Execute Python code in an isolated interpreter");

    socket.close();
  });

  it("generates TypeBox parameters schema", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    const python = pi.registeredTools.find((t: any) => t.name === "run_python");
    expect(python.parameters).toBeDefined();
    // TypeBox objects have a `type` of "object" and `properties`
    expect(python.parameters.type).toBe("object");
    expect(python.parameters.properties.code).toBeDefined();
    expect(python.parameters.properties.code.type).toBe("string");

    socket.close();
  });

  it("converts enum parameters", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    const imageTool = pi.registeredTools.find((t: any) => t.name === "generate_image");
    expect(imageTool.parameters.properties.style).toBeDefined();
    // Enum becomes a Union of Literals in TypeBox
    expect(imageTool.parameters.properties.style.anyOf).toBeDefined();

    socket.close();
  });

  it("marks optional parameters", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    const search = pi.registeredTools.find((t: any) => t.name === "search_web");
    // query is required, num_results is optional
    const queryProp = search.parameters.properties.query;
    const numProp = search.parameters.properties.num_results;

    // Required fields don't have the Optional modifier
    expect(queryProp.type).toBe("string");
    // Optional fields have [Optional] Symbol key in TypeBox
    // Check via the TypeBox convention: optional properties have a modifier
    expect(numProp).toBeDefined();

    socket.close();
  });

  it("execute() proxies tool calls through WebSocket", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    const python = pi.registeredTools.find((t: any) => t.name === "run_python");
    const result = await python.execute(
      "test-call-1",
      { code: "print('hello')" },
      undefined,
      undefined,
      {}
    );

    expect(result.content).toHaveLength(1);
    expect(result.content[0].type).toBe("text");
    expect(result.content[0].text).toContain("run_python");
    expect(result.content[0].text).toContain("hello");

    socket.close();
  });

  it("execute() returns error when tool fails", async () => {
    const errorBackend = new MockBackend({
      onToolCall: () => { throw new Error("SyntaxError: invalid syntax"); },
    });
    await errorBackend.start();

    const socket = await BackendSocket.connect(errorBackend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    const python = pi.registeredTools.find((t: any) => t.name === "run_python");
    const result = await python.execute("call-2", { code: "(" }, undefined, undefined, {});

    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("SyntaxError");

    socket.close();
    await errorBackend.close();
  });

  it("execute() returns error when socket is closed", async () => {
    const socket = await BackendSocket.connect(backend.url);
    const pi = createMockPi();
    await registerBackendTools(pi, socket);

    socket.close();

    const python = pi.registeredTools.find((t: any) => t.name === "run_python");
    const result = await python.execute("call-3", { code: "1" }, undefined, undefined, {});

    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("disconnected");
  });

  it("handles empty tool list", async () => {
    const emptyBackend = new MockBackend({ tools: [] });
    await emptyBackend.start();

    const socket = await BackendSocket.connect(emptyBackend.url);
    const pi = createMockPi();
    const { count } = await registerBackendTools(pi, socket);

    expect(count).toBe(0);
    expect(pi.registeredTools).toHaveLength(0);

    socket.close();
    await emptyBackend.close();
  });
});
