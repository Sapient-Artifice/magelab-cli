import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { getConnection } from "./connection.js";
import { BackendSocket, type ConfirmationRequest } from "./websocket.js";
import { registerBackendTools } from "./tools.js";
import { ensureGatewayProvider } from "./gateway.js";
import { registerCommands } from "./commands.js";
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
  // 0a. Register renderer for MageLab response messages
  try {
    // @ts-ignore — resolved at runtime by Pi's jiti loader
    const { Markdown, Container, Spacer } = await import("@mariozechner/pi-tui");

    const id = (t: string) => t;
    const mdTheme = {
      heading: (t: string) => `\x1b[1m${t}\x1b[0m`,
      link: (t: string) => `\x1b[4m${t}\x1b[0m`,
      linkUrl: (t: string) => `\x1b[2m${t}\x1b[0m`,
      code: (t: string) => `\x1b[36m${t}\x1b[0m`,
      codeBlock: id,
      codeBlockBorder: (t: string) => `\x1b[2m${t}\x1b[0m`,
      quote: (t: string) => `\x1b[3m${t}\x1b[0m`,
      quoteBorder: (t: string) => `\x1b[2m${t}\x1b[0m`,
      hr: (t: string) => `\x1b[2m${t}\x1b[0m`,
      listBullet: (t: string) => `\x1b[2m${t}\x1b[0m`,
      bold: (t: string) => `\x1b[1m${t}\x1b[0m`,
      italic: (t: string) => `\x1b[3m${t}\x1b[0m`,
      strikethrough: (t: string) => `\x1b[9m${t}\x1b[0m`,
      underline: (t: string) => `\x1b[4m${t}\x1b[0m`,
    };

    pi.registerMessageRenderer(
      "magelab-response",
      (message: any, _opts: any, _piTheme: any) => {
        const text =
          typeof message.content === "string"
            ? message.content
            : message.content
                ?.filter((c: any) => c.type === "text")
                .map((c: any) => c.text)
                .join("\n") || "";
        const container = new Container();
        container.addChild(new Spacer(1));
        container.addChild(new Markdown(text, 1, 0, mdTheme));
        return container;
      }
    );
  } catch (err) {
    console.error("[magelab] Custom renderer failed:", err);
    // Theme/TUI imports failed — fall back to default rendering
  }

  // 0. Register MageLab skill directories with Pi (no backend needed)
  pi.on("resources_discover", (_event: unknown, _ctx: any) => {
    const skillPaths: string[] = [];
    const userSkills = join(homedir(), "Mage", "Skills");
    if (existsSync(userSkills)) skillPaths.push(userSkills);

    // Project-scoped skills (cwd/.claude/skills/)
    try {
      const projectSkills = join(process.cwd(), ".claude", "skills");
      if (existsSync(projectSkills)) skillPaths.push(projectSkills);
    } catch { /* cwd may not be accessible */ }

    return { skillPaths };
  });

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
  const sessionApproved = new Set<string>(); // tools approved for this session
  let sessionCtx: any = null;

  socket.on("confirmation_request", (msg) => {
    const req = msg as ConfirmationRequest;
    const tool = req.function_name;

    // Check persistent auto-approve list and session-approved set
    if (autoApprove.has(tool) || sessionApproved.has(tool)) {
      socket.send({
        type: "confirmation_response",
        confirmation_id: req.confirmation_id,
        confirmed: true,
        remember: false,
      });
      return;
    }

    // Ask user via Pi's select dialog: Always / This session / Deny
    if (sessionCtx?.hasUI) {
      const detail = req.script
        ? `${tool}: ${req.script}`
        : `${tool}(${JSON.stringify(req.arguments || {}).slice(0, 200)})`;

      sessionCtx.ui
        .select(`Allow ${tool}?  ${detail}`, [
          "Always",
          "This session",
          "No",
        ])
        .then((choice: string | undefined) => {
          if (choice === "Always") {
            autoApprove.add(tool);
            sessionApproved.add(tool);
            socket.send({
              type: "confirmation_response",
              confirmation_id: req.confirmation_id,
              confirmed: true,
              remember: true,
            });
          } else if (choice === "This session") {
            sessionApproved.add(tool);
            socket.send({
              type: "confirmation_response",
              confirmation_id: req.confirmation_id,
              confirmed: true,
              remember: false,
            });
          } else {
            // "No" or dismissed
            socket.send({
              type: "confirmation_response",
              confirmation_id: req.confirmation_id,
              confirmed: false,
              remember: false,
            });
          }
        })
        .catch(() => {
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

  // 5. Tool execution display — show backend agent's tool calls in Pi's status
  socket.on("tool_result", (msg: any) => {
    if (!sessionCtx?.hasUI) return;
    const name = msg.function_name || "tool";
    sessionCtx.ui.setStatus("magelab-tool", undefined); // clear after completion
  });

  socket.on("tool_debug", (msg: any) => {
    if (!sessionCtx?.hasUI) return;
    if (msg.message_type === "tool_call") {
      // Show which tool is being called
      const content = msg.content || "";
      sessionCtx.ui.setStatus("magelab-tool", `running: ${content.slice(0, 60)}`);
    }
  });

  // Also show confirmation_request as tool execution status
  // (the permission gate handles the response, this just shows status)

  // 6. Subagent status display
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

  // 8. Register /magelab command and skill slash commands
  const commandCount = registerCommands(pi, socket);

  // 8b. MageLab mode flag — when true, magelab-backend provider is active
  let magelabMode = true;

  // 9. MageLab-first input routing
  //    Default: user messages go to MageLab's backend agent.
  //    /pi: switch to Pi-native mode (Pi's LLM handles messages).
  //    /magelab: switch back to MageLab mode.
  pi.registerCommand("pi", {
    description: "Switch to Pi-native mode (Pi's LLM handles messages)",
    handler: async (_args: string, ctx: any) => {
      magelabMode = false;
      if (ctx.hasUI) ctx.ui.notify("Switched to Pi mode", "info");
    },
  });

  // Register MageLab backend as a Pi provider using streamSimple.
  // This makes Pi's native UX (user bubble, assistant bubble, streaming)
  // work seamlessly — the backend IS the LLM provider.
  {
    // @ts-ignore — resolved at runtime by Pi's jiti loader
    const { createAssistantMessageEventStream } = await import("@mariozechner/pi-ai");

    pi.registerProvider("magelab-backend", {
      api: "magelab-ws",
      baseUrl: "http://127.0.0.1:11115",
      apiKey: "unused",
      streamSimple: (_model: any, context: any, options?: any) => {
        const stream = createAssistantMessageEventStream();

        (async () => {
          const output: any = {
            role: "assistant",
            content: [],
            api: "magelab-ws",
            provider: "magelab-backend",
            model: "magelab-agent",
            usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
            stopReason: "stop",
            timestamp: Date.now(),
          };

          try {
            stream.push({ type: "start", partial: output });

            // Send the last user message to the backend
            const messages = context.messages || [];
            const lastUser = [...messages].reverse().find((m: any) => m.role === "user");
            const text = lastUser?.content?.[0]?.text || lastUser?.content || "";
            if (text) {
              socket.send({ type: "text", text: typeof text === "string" ? text : JSON.stringify(text) });
            }

            // Stream tokens as they arrive from the backend
            output.content.push({ type: "text", text: "" });
            let textStarted = false;
            const signal = options?.signal;

            await new Promise<void>((resolve, reject) => {
              const timeout = setTimeout(() => reject(new Error("Backend did not respond within 120s")), 120_000);

              // Handle abort (Esc key)
              signal?.addEventListener("abort", () => {
                clearTimeout(timeout);
                socket.send({ type: "control", action: "stop" });
                resolve();
              });

              const finish = () => {
                clearTimeout(timeout);
                if (sessionCtx?.hasUI) {
                  sessionCtx.ui.setStatus("magelab-tool", undefined);
                  sessionCtx.ui.setStatus("magelab-subagent", undefined);
                }
                if (textStarted) {
                  stream.push({ type: "text_end", contentIndex: 0, content: output.content[0].text, partial: output });
                }
                resolve();
              };

              // Streaming tokens (when backend has stream: true)
              socket.on("assistant_stream", (msg: any) => {
                if (msg.phase === "start") {
                  textStarted = true;
                  stream.push({ type: "text_start", contentIndex: 0, partial: output });
                } else if (msg.phase === "delta" && msg.token) {
                  output.content[0].text += msg.token;
                  stream.push({ type: "text_delta", contentIndex: 0, delta: msg.token, partial: output });
                } else if (msg.phase === "end") {
                  // Stream end — finish
                  finish();
                }
              });

              // Non-streaming (backend sends complete text at once)
              socket.on("assistant", (msg: any) => {
                if (msg.text) {
                  output.content[0].text = msg.text;
                  if (!textStarted) {
                    stream.push({ type: "text_start", contentIndex: 0, partial: output });
                    textStarted = true;
                  }
                  stream.push({ type: "text_delta", contentIndex: 0, delta: msg.text, partial: output });
                  finish();
                }
              });

              // Tool execution status — use setWorkingMessage which
              // is visible during streaming (setStatus is not)
              socket.on("confirmation_request", (msg: any) => {
                if (sessionCtx?.hasUI) {
                  sessionCtx.ui.setWorkingMessage(`Running ${msg.function_name}...`);
                }
              });

              socket.on("tool_result", (msg: any) => {
                if (sessionCtx?.hasUI) {
                  const name = msg.function_name || "tool";
                  sessionCtx.ui.setWorkingMessage(`${name} done`);
                }
              });

              socket.on("tool_debug", (msg: any) => {
                if (sessionCtx?.hasUI && msg.content) {
                  sessionCtx.ui.setWorkingMessage(msg.content.slice(0, 80));
                }
              });

              // Subagent progress
              socket.on("subagent_update", (msg: any) => {
                if (sessionCtx?.hasUI) {
                  const label = msg.progress
                    ? `${msg.name}: ${msg.progress}`
                    : `${msg.name}: ${msg.status || "running"}`;
                  sessionCtx.ui.setWorkingMessage(label);
                }
              });

              socket.on("subagent_complete", (msg: any) => {
                if (sessionCtx?.hasUI) {
                  sessionCtx.ui.setWorkingMessage(`${msg.name}: done`);
                }
              });

              // Done signal (streaming mode)
              socket.on("assistant_complete", () => {
                finish();
              });
            });

            stream.push({ type: "done", reason: "stop", message: output });
            stream.end();
          } catch (err: any) {
            output.stopReason = "error";
            output.errorMessage = err.message;
            stream.push({ type: "error", reason: "error", error: output });
            stream.end();
          }
        })();

        return stream;
      },
      models: [
        {
          id: "magelab-agent",
          name: "MageLab Agent",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 128000,
          maxTokens: 16384,
        },
      ],
    });
  }

  // 10. Activate only the MageLab tools we just registered on session start.

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
      ctx.ui.notify(
        `MageLab: ${toolCount} tools, ${commandCount} commands (${conn.mode})`,
        "info"
      );

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
