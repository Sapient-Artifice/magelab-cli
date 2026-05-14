import { describe, it, expect, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { WebSocketServer } from "ws";
import { BackendSocket } from "../src/websocket.js";
import { MockBackend } from "./mock-backend.js";

// Test the permission gate behavior by simulating confirmation_request flow

let cleanups: (() => void)[] = [];

afterEach(() => {
  for (const fn of cleanups) fn();
  cleanups = [];
});

describe("permission gate", () => {
  it("auto-approves tools in the confirmation handler", async () => {
    // Create a backend that requires confirmation
    const backend = new MockBackend({ requireConfirmation: true });
    await backend.start();
    cleanups.push(() => backend.close());

    const socket = await BackendSocket.connect(backend.url);
    cleanups.push(() => socket.close());

    // Register an auto-approve handler (simulating the extension)
    const responses: any[] = [];
    socket.on("confirmation_request", (msg: any) => {
      responses.push({ action: "auto-approved", tool: msg.function_name });
      socket.send({
        type: "confirmation_response",
        confirmation_id: msg.confirmation_id,
        confirmed: true,
        remember: false,
      });
    });

    const result = await socket.callTool("read_file", { path: "/tmp/test" });
    expect(result.success).toBe(true);
    expect(responses).toHaveLength(1);
    expect(responses[0].action).toBe("auto-approved");
  });

  it("denied confirmation returns error result", async () => {
    const backend = new MockBackend({ requireConfirmation: true });
    await backend.start();
    cleanups.push(() => backend.close());

    const socket = await BackendSocket.connect(backend.url);
    cleanups.push(() => socket.close());

    // Deny all confirmations
    socket.on("confirmation_request", (msg: any) => {
      socket.send({
        type: "confirmation_response",
        confirmation_id: msg.confirmation_id,
        confirmed: false,
        remember: false,
      });
    });

    const result = await socket.callTool("run_bash", { command: "rm -rf /" });
    expect(result.success).toBe(false);
    expect(result.error).toContain("denied");
  });

  it("selective approval based on tool name", async () => {
    const backend = new MockBackend({ requireConfirmation: true });
    await backend.start();
    cleanups.push(() => backend.close());

    const socket = await BackendSocket.connect(backend.url);
    cleanups.push(() => socket.close());

    const autoApprove = new Set(["read_file", "search_files"]);

    socket.on("confirmation_request", (msg: any) => {
      const confirmed = autoApprove.has(msg.function_name);
      socket.send({
        type: "confirmation_response",
        confirmation_id: msg.confirmation_id,
        confirmed,
        remember: false,
      });
    });

    // Auto-approved tool should succeed
    const r1 = await socket.callTool("read_file", { path: "/tmp/x" });
    expect(r1.success).toBe(true);

    // Non-approved tool should be denied
    const r2 = await socket.callTool("run_bash", { command: "ls" });
    expect(r2.success).toBe(false);
  });
});
