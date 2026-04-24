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
  socket: BackendSocket
): number {
  // 1. /magelab <prompt> — delegate to backend agent
  pi.registerCommand("magelab", {
    description: "Send a prompt to MageLab's agent (backend LLM + tools)",
    handler: async (args: string, ctx: any) => {
      if (!args.trim()) {
        if (ctx.hasUI) ctx.ui.notify("Usage: /magelab <prompt>", "warning");
        return;
      }
      // Send as a text message to the backend's agentic loop
      socket.send({ type: "text", text: args });
      if (ctx.hasUI) {
        ctx.ui.setStatus("magelab-agent", "MageLab agent thinking...");
      }
    },
  });

  // 2. Discover and register skill commands
  const skillCommands = discoverSkillCommands();
  for (const cmd of skillCommands) {
    pi.registerCommand(cmd.name, {
      description: cmd.description,
      handler: async (args: string, _ctx: any) => {
        // Combine command body with user args and send to backend
        const prompt = args ? `${cmd.body}\n\n${args}` : cmd.body;
        socket.send({ type: "text", text: prompt });
      },
    });
  }

  return 1 + skillCommands.length; // /magelab + skill commands
}
