/**
 * Register MageLab slash commands with Pi.
 *
 * 1. /magelab <prompt> — delegate a full conversation to the backend's
 *    agentic loop (LLM + tool execution + subagents). Streams results
 *    back to Pi.
 *
 * 2. Skill commands — discovered from commands/*.md files in activated
 *    skill directories. Each becomes a Pi /command that sends the
 *    command body as a prompt to the backend.
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import type { BackendSocket, ServerMessage } from "./websocket.js";
import type { MageClient } from "./client/index.js";

interface SkillCommand {
  name: string;
  description: string;
  body: string;
}

/** Parse frontmatter + body from a markdown file */
function parseMarkdown(content: string): { frontmatter: Record<string, string>; body: string } {
  const match = content.match(/^---\s*\n([\s\S]*?)\n---\s*\n([\s\S]*)$/);
  if (!match) return { frontmatter: {}, body: content };

  const fm: Record<string, string> = {};
  for (const line of match[1].split("\n")) {
    const kv = line.match(/^(\w[\w-]*)\s*:\s*"?(.+?)"?\s*$/);
    if (kv) fm[kv[1]] = kv[2];
  }
  return { frontmatter: fm, body: match[2].trim() };
}

/** Discover commands/*.md files from MageLab skill directories */
function discoverSkillCommands(): SkillCommand[] {
  const commands: SkillCommand[] = [];
  const skillDirs = [
    join(homedir(), "Mage", "Skills"),
  ];

  // Add project-scoped skills
  try {
    const projectSkills = join(process.cwd(), ".claude", "skills");
    if (existsSync(projectSkills)) skillDirs.push(projectSkills);
  } catch { /* */ }

  for (const skillsRoot of skillDirs) {
    if (!existsSync(skillsRoot)) continue;
    try {
      // Walk: skillsRoot/*/commands/*.md and skillsRoot/*/*/commands/*.md
      for (const entry of readdirSync(skillsRoot, { withFileTypes: true })) {
        if (!entry.isDirectory()) continue;
        scanCommandsDir(join(skillsRoot, entry.name, "commands"), commands);
        // Namespaced: skillsRoot/namespace/skill/commands/
        try {
          for (const sub of readdirSync(join(skillsRoot, entry.name), { withFileTypes: true })) {
            if (!sub.isDirectory()) continue;
            scanCommandsDir(join(skillsRoot, entry.name, sub.name, "commands"), commands);
          }
        } catch { /* */ }
      }
    } catch { /* */ }
  }

  return commands;
}

function scanCommandsDir(dir: string, out: SkillCommand[]): void {
  if (!existsSync(dir)) return;
  try {
    for (const file of readdirSync(dir)) {
      if (!file.endsWith(".md")) continue;
      const content = readFileSync(join(dir, file), "utf-8");
      const { frontmatter, body } = parseMarkdown(content);
      const name = file.replace(/\.md$/, "");
      out.push({
        name,
        description: frontmatter.description || `MageLab skill command: ${name}`,
        body,
      });
    }
  } catch { /* */ }
}

/**
 * Register all commands with Pi.
 */
export function registerCommands(
  pi: any,
  socket: BackendSocket,
  client: MageClient
): number {
  // 1. /magelab <prompt> — delegate to backend agent
  pi.registerCommand("magelab", {
    description: "Send a prompt to MageLab's agent (backend LLM + tools)",
    handler: async (args: string, ctx: any) => {
      if (!args.trim()) {
        if (ctx.hasUI) ctx.ui.notify("Usage: /magelab <prompt>", "warning");
        return;
      }
      if (ctx.hasUI) {
        ctx.ui.setStatus("magelab-agent", "MageLab agent thinking...");
      }
      try {
        const result = await client.runTurn({ text: args }).completed;
        pi.sendMessage({
          customType: "magelab-response",
          content: result.text,
          display: true,
        });
        if (result.status === "error" && ctx.hasUI) {
          ctx.ui.notify(result.error || "MageLab turn failed", "error");
        }
      } catch (error) {
        if (ctx.hasUI) ctx.ui.notify((error as Error).message, "error");
      } finally {
        if (ctx.hasUI) ctx.ui.setStatus("magelab-agent", undefined);
      }
    },
  });

  // 2. /chats — list backend chat histories
  pi.registerCommand("chats", {
    description: "List MageLab chat histories",
    handler: async (_args: string, ctx: any) => {
      try {
        const result = await socket.requestByType(
          { type: "get_chats" },
          "chat_list_result"
        );
        const chats = (result as any).chats || [];
        if (chats.length === 0) {
          if (ctx.hasUI) ctx.ui.notify("No chat histories found", "info");
          return;
        }
        const list = chats
          .map((path: string) => {
            const name = path.split("/").pop() || path;
            return `  ${name}`;
          })
          .join("\n");
        pi.sendMessage({
          customType: "magelab-agent",
          content: `**Chat histories:**\n${list}`,
          display: true,
        });
      } catch {
        if (ctx.hasUI) ctx.ui.notify("Failed to list chats", "error");
      }
    },
  });

  // 3. /chat <name> — switch backend chat history
  pi.registerCommand("chat", {
    description: "Switch MageLab chat by numeric id (use /chats to list)",
    handler: async (args: string, ctx: any) => {
      const chatId = Number.parseInt(args.trim(), 10);
      if (!Number.isInteger(chatId) || chatId <= 0) {
        if (ctx.hasUI) ctx.ui.notify("Usage: /chat <chat_id>", "warning");
        return;
      }
      try {
        const result = await socket.requestById(
          { type: "set_chat", chat_id: chatId },
          "chat_switch_result"
        );
        const r = result as any;
        if (r.ok) {
          if (ctx.hasUI) ctx.ui.notify(`Switched to chat: ${r.chat_id || chatId}`, "info");
        } else {
          if (ctx.hasUI) ctx.ui.notify(`Failed: ${r.error || "unknown error"}`, "error");
        }
      } catch {
        if (ctx.hasUI) ctx.ui.notify("Failed to switch chat", "error");
      }
    },
  });

  // 4. /newchat — start a new backend chat
  pi.registerCommand("newchat", {
    description: "Start a new MageLab chat history",
    handler: async (_args: string, ctx: any) => {
      try {
        const result = await socket.requestById(
          { type: "new_chat" },
          "new_chat_result"
        );
        const chatId = (result as any).chat_id;
        if (ctx.hasUI) ctx.ui.notify(`New chat: ${chatId || "created"}`, "info");
      } catch {
        if (ctx.hasUI) ctx.ui.notify("Failed to create new chat", "error");
      }
    },
  });

  // 5. /backend-model <name> — switch backend LLM model
  //    Pi has a built-in /model command, so we use a different name.
  pi.registerCommand("backend-model", {
    description: "Show or switch MageLab backend model",
    handler: async (args: string, ctx: any) => {
      const model = args.trim();
      if (!model) {
        // Show current model from runtime config
        try {
          const config = await socket.requestByType(
            { type: "get_runtime_config" },
            "runtime_config"
          );
          const c = config as any;
          if (ctx.hasUI) {
            ctx.ui.notify(`Backend model: ${c.llm_model_name || "unknown"}`, "info");
          }
        } catch {
          if (ctx.hasUI) ctx.ui.notify("Failed to get model info", "error");
        }
        return;
      }
      try {
        const result = await socket.requestByType(
          { type: "set_model", model },
          "set_model_result"
        );
        const r = result as any;
        if (r.success) {
          if (ctx.hasUI) ctx.ui.notify(`Backend model set to: ${r.model || model}`, "info");
        } else {
          if (ctx.hasUI) ctx.ui.notify(`Failed to set model: ${model}`, "error");
        }
      } catch {
        if (ctx.hasUI) ctx.ui.notify("Failed to set model", "error");
      }
    },
  });

  // 6. Discover and register skill commands
  const skillCommands = discoverSkillCommands();
  for (const cmd of skillCommands) {
    pi.registerCommand(cmd.name, {
      description: cmd.description,
      handler: async (args: string, _ctx: any) => {
        // Combine command body with user args and send to backend
        const prompt = args ? `${cmd.body}\n\n${args}` : cmd.body;
        const result = await client.runTurn({ text: prompt }).completed;
        pi.sendMessage({
          customType: "magelab-response",
          content: result.text,
          display: true,
        });
      },
    });
  }

  return 5 + skillCommands.length; // /magelab + /chats + /chat + /newchat + /model + skill commands
}
