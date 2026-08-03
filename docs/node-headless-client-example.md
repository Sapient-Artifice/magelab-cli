# Persistent Node.js Headless Client Example

Use the reusable client for long-lived Node.js services. Running `mage connect`
once at process startup is a convenient way to reuse CLI authentication and
backend discovery; do not spawn `mage ask` for each application request.

```ts
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { MageClient } from "@magelab/agent/client";

const execFileAsync = promisify(execFile);

async function discoverMage() {
  const { stdout } = await execFileAsync("mage", [
    "connect",
    "--json",
    "--no-launch",
  ]);
  const connection = JSON.parse(stdout);
  if (!connection.url || !["local", "relay"].includes(connection.mode)) {
    throw new Error("No Mage WebSocket is available");
  }
  return connection as {
    url: string;
    token: string | null;
    mode: "local" | "relay";
  };
}

const connection = await discoverMage();
const httpBaseUrl =
  connection.mode === "local"
    ? connection.url.replace(/^ws/, "http").replace(/\/ws$/, "")
    : undefined;

const mage = await MageClient.connect({
  wsUrl: connection.url,
  token: connection.token,
  httpBaseUrl,
  observe(event) {
    // Metadata only: request id, status, queue wait, and duration.
    metrics.record(event);
  },
});

// Persist these Mage identifiers beside the application's conversation record.
const session = await mage.createSession({
  name: "CRM conversation",
  state: {
    mcps: { enabled_servers: ["pipedrive"] },
  },
});

const turn = mage.runConversationTurn({
  text: "Fetch deal 2 from Pipedrive",
  setup: {
    session_id: session.id,
    llm_model_name: "model-name",
    system_message: "You are the CRM assistant.",
    mcps: { enabled_servers: ["pipedrive"] },
    createChat: true,
  },
});

for await (const event of turn.events) {
  if (event.type === "assistant_stream" && event.phase === "delta") {
    sendChunkToApplication(event.token ?? "");
  }
}

const result = await turn.completed;
if (result.status !== "completed") {
  throw new Error(result.error ?? `Mage turn ${result.status}`);
}
```

## Application responsibilities

The host service must:

- authenticate its caller;
- map its conversation identifier to Mage `session_id` and `chat_id`;
- persist that mapping with the Mage deployment identity;
- authorize requested models, prompts, MCP servers, tools, and files;
- derive any future `tenant_id` from trusted authenticated identity; and
- decide whether an unknown-outcome turn after disconnect may be retried.

The client never replays a submitted prompt automatically. A disconnect before
`assistant_complete` leaves the outcome unknown because model or tool side
effects may already have occurred.

Clients created with `MageClient.connect` for the same normalized WebSocket URL
share a serialization coordinator within one Node.js process. Multiple service
processes or replicas targeting one Mage process still require an injected
distributed coordinator until the backend advertises request-scoped isolation.
