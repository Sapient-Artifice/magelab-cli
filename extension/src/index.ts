import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { getConnection } from "./connection.js";
import { BackendSocket, type ConfirmationRequest } from "./websocket.js";
import { registerBackendTools } from "./tools.js";
import { ensureGatewayProvider } from "./gateway.js";
import { isAllowedUrl, isAllowedFilepath } from "./validation.js";

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
  } catch (err) {
    console.warn(`[magelab] Gateway provider setup failed: ${(err as Error).message ?? err}`);
  }

  // 2. Get connection info from magelab CLI.
  // Retry up to 5 times (1.5 s apart) to allow a just-launched backend to
  // become ready.  Only bail out immediately on hard failures (binary not
  // found).  Transient subprocess errors or mode=="none" are retried.
  let conn;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      conn = await getConnection();
      if (conn.mode !== "none") break;
    } catch (err: any) {
      // Binary not found — no point retrying.
      if (err?.code === "ENOENT" || (err?.message as string)?.includes("not found")) return;
      // Any other error (e.g. subprocess crash) — retry.
    }
    if (attempt < 4) await new Promise((r) => setTimeout(r, 1500));
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
    const req = msg as ConfirmationRequest;

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
      // No UI available — deny by default for safety (headless mode)
      socket.send({
        type: "confirmation_response",
        confirmation_id: req.confirmation_id,
        confirmed: false,
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

  // 5. Subagent status display
  socket.on("subagent_update", (msg: any) => {
    if (!sessionCtx?.hasUI) return;
    const label = msg.progress
      ? `${msg.name}: ${msg.progress}`
      : `${msg.name}: ${msg.status || "running"}`;
    sessionCtx.ui.setStatus("magelab-subagent", label);
  });

  socket.on("subagent_complete", (msg: any) => {
    if (!sessionCtx?.hasUI) return;
    sessionCtx.ui.setStatus("magelab-subagent", undefined); // clear status
    const level = msg.error ? "error" : "info";
    const detail = msg.error || msg.result || msg.status || "done";
    sessionCtx.ui.notify(`Subagent ${msg.name}: ${detail}`, level);
  });

  // 6. Backend notifications (Notify, OpenUrl, OpenFile)
  socket.on("notify", (msg: any) => {
    if (!sessionCtx?.hasUI) return;
    sessionCtx.ui.notify(`${msg.title}: ${msg.body}`, "info");
  });

  socket.on("open_url", (msg: any) => {
    if (!isAllowedUrl(msg.url)) return;
    import("open").then((m) => m.default(msg.url)).catch(() => {});
  });

  socket.on("open_file", (msg: any) => {
    if (!isAllowedFilepath(msg.filepath)) return;
    import("open").then((m) => m.default(msg.filepath)).catch(() => {});
  });

  // 7. Fetch and register backend tools
  const magelabToolNames = new Set<string>();
  let toolCount: number;
  try {
    const result = await registerBackendTools(pi, socket);
    toolCount = result.count;
    for (const n of result.names) magelabToolNames.add(n);
  } catch {
    socket.close();
    return;
  }

  // 8. Activate only the MageLab tools we just registered on session start.

  pi.on("session_start", async (_event: unknown, ctx: any) => {
    sessionCtx = ctx;
    try {
      const active: string[] = pi.getActiveTools();
      const toActivate = [...magelabToolNames].filter((n) => !active.includes(n));
      if (toActivate.length > 0) {
        pi.setActiveTools([...active, ...toActivate]);
      }
    } catch {
      // Tools registered but activation failed — they'll still appear in getAllTools
    }
    if (ctx.hasUI) {
      ctx.ui.notify(`MageLab: ${toolCount} tools active (${conn.mode})`, "info");

      // Proactive balance check — warn if low or zero
      try {
        const { execFile: ef } = await import("node:child_process");
        const { promisify: p } = await import("node:util");
        const { existsSync: ex } = await import("node:fs");
        const { join: j } = await import("node:path");
        const { homedir: hd } = await import("node:os");
        const cargoPath = j(hd(), ".cargo", "bin", "magelab");
        const bin = ex(cargoPath) ? cargoPath : "magelab";
        const { stdout } = await p(ef)(bin, ["balance"]);
        const balanceMatch = stdout.match(/(\$[\d.]+|\d+\.\d+)/);
        if (balanceMatch) {
          const amount = parseFloat(balanceMatch[0].replace("$", ""));
          if (amount <= 0) {
            ctx.ui.notify(
              "MageLab: no credits remaining. Add credits at magelab.ai",
              "warning"
            );
          } else if (amount < 1) {
            ctx.ui.notify(
              `MageLab: low balance ($${amount.toFixed(2)}). Add credits at magelab.ai`,
              "warning"
            );
          }
        }
      } catch {
        // Balance check failed — non-fatal
      }
    }
  });

  // 9. Intercept Gateway errors with helpful messages
  //    Only show MageLab-specific guidance when the active model is from our provider.
  pi.on("after_provider_response", async (event: any, ctx: any) => {
    if (!ctx.hasUI) return;
    // Check if current model is from the magelab provider
    const model = ctx.model;
    const isMagelab =
      model?.provider === "magelab" ||
      model?.providerId === "magelab" ||
      model?.id?.startsWith("claude-") ||
      model?.id?.startsWith("gpt-") ||
      model?.id?.startsWith("models/gemini");
    if (!isMagelab) return;

    if (event.status === 402) {
      ctx.ui.notify(
        "MageLab: no credits remaining. Add credits at magelab.ai or run: magelab balance",
        "error"
      );
    } else if (event.status === 401) {
      ctx.ui.notify(
        "MageLab: authentication failed. Run: magelab login",
        "error"
      );
    } else if (event.status === 429) {
      ctx.ui.notify(
        "MageLab: rate limited. Wait a moment and try again.",
        "warning"
      );
    }
  });

  // 10. Clean shutdown
  pi.on("session_shutdown", async () => {
    socket.close();
  });
}
