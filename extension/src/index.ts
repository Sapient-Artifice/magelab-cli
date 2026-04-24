import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { getConnection } from "./connection.js";
import { BackendSocket, type ConfirmationRequestMessage } from "./websocket.js";
import { registerBackendTools } from "./tools.js";
import { ensureGatewayProvider } from "./gateway.js";

/** Read auto_approve list from magelab CLI config */
function loadAutoApproveList(): Set<string> {
  const paths = [
    join(homedir(), "Library", "Application Support", "magelab", "cli.toml"),
    join(homedir(), ".config", "magelab", "cli.toml"),
  ];
  for (const p of paths) {
    if (!existsSync(p)) continue;
    try {
      const content = readFileSync(p, "utf-8");
      const match = content.match(/auto_approve\s*=\s*\[([\s\S]*?)\]/);
      if (match) {
        const items = match[1].match(/"([^"]+)"/g);
        if (items) {
          return new Set(items.map((s) => s.replace(/"/g, "")));
        }
      }
    } catch { /* ignore */ }
  }
  return new Set();
}

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

  // 4. Permission gate for tool confirmations
  const autoApprove = loadAutoApproveList();
  let sessionCtx: any = null;

  socket.on("confirmation_request", (msg) => {
    const req = msg as ConfirmationRequestMessage;

    if (autoApprove.has(req.function_name)) {
      // Auto-approved tool — no prompt needed
      socket.send({
        type: "confirmation_response",
        confirmation_id: req.confirmation_id,
        confirmed: true,
        remember: false,
      });
      return;
    }

    // Ask user via Pi's confirm dialog
    if (sessionCtx?.hasUI) {
      const detail = req.script
        ? `${req.function_name}: ${req.script}`
        : `${req.function_name}(${JSON.stringify(req.arguments || {}).slice(0, 200)})`;

      sessionCtx.ui
        .confirm(`Allow ${req.function_name}?`, detail)
        .then((confirmed: boolean) => {
          socket.send({
            type: "confirmation_response",
            confirmation_id: req.confirmation_id,
            confirmed,
            remember: false,
          });
        })
        .catch(() => {
          // Dialog dismissed — deny
          socket.send({
            type: "confirmation_response",
            confirmation_id: req.confirmation_id,
            confirmed: false,
            remember: false,
          });
        });
    } else {
      // No UI available — auto-approve (headless mode)
      socket.send({
        type: "confirmation_response",
        confirmation_id: req.confirmation_id,
        confirmed: true,
        remember: false,
      });
    }
  });
  socket.onStateChange((state) => {
    if (!sessionCtx?.hasUI) return;
    if (state === "reconnecting") {
      sessionCtx.ui.notify("MageLab backend disconnected — reconnecting...", "warning");
    } else if (state === "connected") {
      sessionCtx.ui.notify("MageLab backend reconnected", "info");
    } else if (state === "disconnected") {
      sessionCtx.ui.notify("MageLab backend disconnected. Restart Pi to reconnect.", "error");
    }
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
    sessionCtx = ctx;
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
