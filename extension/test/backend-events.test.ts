import { describe, it, expect, afterEach } from "vitest";
import { WebSocketServer, WebSocket as WsWebSocket } from "ws";
import { BackendSocket } from "../src/websocket.js";

let cleanups: (() => void)[] = [];

afterEach(() => {
  for (const fn of cleanups) fn();
  cleanups = [];
});

function startServer(): Promise<{ url: string; broadcast: (msg: any) => void; close: () => void }> {
  return new Promise((resolve) => {
    const clients: WsWebSocket[] = [];
    const server = new WebSocketServer({ port: 0 });
    server.on("listening", () => {
      const port = (server.address() as any).port;
      resolve({
        url: `ws://127.0.0.1:${port}`,
        broadcast: (msg: any) => {
          for (const ws of clients) {
            ws.send(JSON.stringify(msg));
          }
        },
        close: () => server.close(),
      });
    });
    server.on("connection", (ws) => {
      clients.push(ws);
    });
  });
}

describe("backend events (Phase 3)", () => {
  it("receives subagent_update messages", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const updates: any[] = [];
    socket.on("subagent_update", (msg) => updates.push(msg));

    broadcast({
      type: "subagent_update",
      task_id: "task-1",
      name: "research",
      status: "running",
      progress: "Searching files...",
    });

    await new Promise((r) => setTimeout(r, 50));

    expect(updates).toHaveLength(1);
    expect(updates[0].name).toBe("research");
    expect(updates[0].progress).toBe("Searching files...");
  });

  it("receives subagent_complete messages", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const completions: any[] = [];
    socket.on("subagent_complete", (msg) => completions.push(msg));

    broadcast({
      type: "subagent_complete",
      task_id: "task-1",
      name: "research",
      status: "done",
      result: "Found 3 matches",
    });

    await new Promise((r) => setTimeout(r, 50));

    expect(completions).toHaveLength(1);
    expect(completions[0].name).toBe("research");
    expect(completions[0].result).toBe("Found 3 matches");
  });

  it("receives subagent_complete with error", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const completions: any[] = [];
    socket.on("subagent_complete", (msg) => completions.push(msg));

    broadcast({
      type: "subagent_complete",
      task_id: "task-2",
      name: "failing-agent",
      error: "Timeout after 60s",
    });

    await new Promise((r) => setTimeout(r, 50));

    expect(completions).toHaveLength(1);
    expect(completions[0].error).toBe("Timeout after 60s");
  });

  it("receives notify messages", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const notifications: any[] = [];
    socket.on("notify", (msg) => notifications.push(msg));

    broadcast({
      type: "notify",
      title: "MCP Server",
      body: "Connected to filesystem server",
    });

    await new Promise((r) => setTimeout(r, 50));

    expect(notifications).toHaveLength(1);
    expect(notifications[0].title).toBe("MCP Server");
    expect(notifications[0].body).toBe("Connected to filesystem server");
  });

  it("receives open_url messages", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const urls: any[] = [];
    socket.on("open_url", (msg) => urls.push(msg));

    broadcast({
      type: "open_url",
      url: "https://docs.magelab.ai",
    });

    await new Promise((r) => setTimeout(r, 50));

    expect(urls).toHaveLength(1);
    expect(urls[0].url).toBe("https://docs.magelab.ai");
  });

  it("receives open_file messages", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const files: any[] = [];
    socket.on("open_file", (msg) => files.push(msg));

    broadcast({
      type: "open_file",
      filepath: "/Users/test/project/README.md",
    });

    await new Promise((r) => setTimeout(r, 50));

    expect(files).toHaveLength(1);
    expect(files[0].filepath).toBe("/Users/test/project/README.md");
  });

  it("handles multiple subagent updates in sequence", async () => {
    const { url, broadcast, close } = await startServer();
    cleanups.push(close);

    const socket = await BackendSocket.connect(url);
    cleanups.push(() => socket.close());

    const updates: any[] = [];
    const completions: any[] = [];
    socket.on("subagent_update", (msg) => updates.push(msg));
    socket.on("subagent_complete", (msg) => completions.push(msg));

    broadcast({ type: "subagent_update", task_id: "t1", name: "agent-a", progress: "Step 1" });
    broadcast({ type: "subagent_update", task_id: "t1", name: "agent-a", progress: "Step 2" });
    broadcast({ type: "subagent_complete", task_id: "t1", name: "agent-a", result: "Done" });

    await new Promise((r) => setTimeout(r, 50));

    expect(updates).toHaveLength(2);
    expect(completions).toHaveLength(1);
  });
});
