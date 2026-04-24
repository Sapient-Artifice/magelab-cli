import { getConnection } from "./connection.js";
import { BackendSocket, type ConfirmationRequestMessage } from "./websocket.js";
import { registerBackendTools } from "./tools.js";
import { ensureGatewayProvider } from "./gateway.js";

export default async function (pi: any) {
  // 1. Configure MageLab Gateway as a Pi provider (if logged in)
  try {
    await ensureGatewayProvider();
  } catch {
    // Non-fatal — user can configure providers manually
  }

  // 2. Get connection info from magelab CLI (retry up to 5 times for backend startup)
  let conn;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      conn = await getConnection();
      if (conn.mode !== "none") break;
    } catch {
      return;
    }
    await new Promise((r) => setTimeout(r, 1500));
  }

  if (!conn || conn.mode === "none" || conn.mode === "remote") {
    return;
  }

  // 3. Connect WebSocket to backend (local or relay)
  let socket: BackendSocket;
  try {
    socket = await BackendSocket.connect(conn.url!, conn.token);
  } catch {
    return;
  }

  // 4. Auto-approve all confirmation requests (MVP — no permission gate)
  socket.on("confirmation_request", (msg) => {
    const req = msg as ConfirmationRequestMessage;
    socket.send({
      type: "confirmation_response",
      confirmation_id: req.confirmation_id,
      confirmed: true,
      remember: false,
    });
  });

  // 5. Fetch and register backend tools
  let toolCount: number;
  try {
    toolCount = await registerBackendTools(pi, socket);
  } catch {
    socket.close();
    return;
  }

  // 6. Activate extension tools on session start (can't call setActiveTools during loading)
  pi.on("session_start", async (_event: unknown, ctx: any) => {
    try {
      const active = pi.getActiveTools();
      const all = pi.getAllTools();
      const newTools = all
        .map((t: any) => t.name)
        .filter((name: string) => !active.includes(name));
      if (newTools.length > 0) {
        pi.setActiveTools([...active, ...newTools]);
      }
    } catch {
      // Tools registered but activation failed — they'll still appear in getAllTools
    }
    if (ctx.hasUI) {
      ctx.ui.notify(`MageLab: ${toolCount} tools active (${conn.mode})`, "info");
    }
  });

  // 7. Clean shutdown
  pi.on("session_shutdown", async () => {
    socket.close();
  });
}
