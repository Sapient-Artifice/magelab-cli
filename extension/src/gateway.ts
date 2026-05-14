/**
 * Configure Pi to use MageLab Gateway as an LLM provider.
 *
 * Writes ~/.pi/agent/models.json with the Gateway as an OpenAI-compatible
 * provider. Uses a static API key (preferred) so the stored credential never
 * expires. Falls back to a short-lived JWT only when no API key is available,
 * in which case the provider entry is NOT persisted so it is refreshed on
 * every startup.
 */
import { execFile } from "node:child_process";
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

const GATEWAY_URL = "https://api.magelab.ai/v1";
const PROVIDER_NAME = "magelab";

/** Find the magelab binary: ~/.cargo/bin/ first, then PATH */
function findBinary(): string {
  const cargoPath = join(homedir(), ".cargo", "bin", "magelab");
  if (existsSync(cargoPath)) return cargoPath;
  return "magelab";
}

/**
 * Try to read a static API key from the CLI config file.
 * Returns undefined if no api_key is configured.
 * Only reads TOML double-quoted strings (the only format magelab writes).
 */
function readStaticApiKey(): string | undefined {
  const configPaths = [
    // macOS
    join(homedir(), "Library", "Application Support", "magelab", "cli.toml"),
    // Linux / Windows (XDG / fallback)
    join(homedir(), ".config", "magelab", "cli.toml"),
  ];
  for (const p of configPaths) {
    if (!existsSync(p)) continue;
    try {
      const content = readFileSync(p, "utf-8");
      // Match both double-quoted and single-quoted TOML values
      const match = content.match(/^api_key\s*=\s*["']([^"']+)["']/m);
      if (match) return match[1];
    } catch {
      // Unreadable config — try next path
    }
  }
  return undefined;
}

/**
 * Get the best available credential for authenticating to the Gateway.
 * Returns { key, isJwt } where isJwt=true means the key is a short-lived JWT
 * and must NOT be persisted to models.json.
 */
async function getCredential(): Promise<{ key: string; isJwt: boolean }> {
  // 1. Environment variable (static API key — always preferred)
  const envKey = process.env.MAGELAB_API_KEY;
  if (envKey) return { key: envKey, isJwt: false };

  // 2. Static API key from config file
  const staticKey = readStaticApiKey();
  if (staticKey) return { key: staticKey, isJwt: false };

  // 3. Fall back to a short-lived JWT from `magelab auth token`
  //    This token expires in minutes — callers must NOT persist it.
  const bin = findBinary();
  const { stdout } = await execFileAsync(bin, ["auth", "token"]);
  const token = stdout.trim();
  if (token) return { key: token, isJwt: true };

  throw new Error("No API key or auth token found — run: magelab login");
}

/**
 * Ensure ~/.pi/agent/models.json has the MageLab Gateway configured.
 * Merges with any existing providers the user has set up.
 */
export async function ensureGatewayProvider(): Promise<boolean> {
  const piDir = join(homedir(), ".pi", "agent");
  const modelsPath = join(piDir, "models.json");

  // Load existing models.json or start fresh
  let config: any = { providers: {} };
  if (existsSync(modelsPath)) {
    try {
      config = JSON.parse(readFileSync(modelsPath, "utf-8"));
      if (!config.providers) config.providers = {};
    } catch {
      config = { providers: {} };
    }
  }

  // Skip if already configured with a static (non-JWT) API key.
  // If previously stored with a JWT, re-configure so it gets refreshed.
  if (config.providers[PROVIDER_NAME]) {
    const stored = config.providers[PROVIDER_NAME];
    // Heuristic: JWTs are long (>200 chars) and contain two dots; API keys are shorter.
    const looksLikeJwt =
      typeof stored.apiKey === "string" &&
      stored.apiKey.length > 200 &&
      stored.apiKey.split(".").length === 3;
    if (!looksLikeJwt) return true; // Static key already stored — nothing to do.
    // Fall through to replace the stale JWT entry.
    delete config.providers[PROVIDER_NAME];
  }

  // Obtain credential — static key preferred, JWT fallback.
  let cred: { key: string; isJwt: boolean };
  try {
    cred = await getCredential();
  } catch {
    return false; // Can't configure without any credential
  }

  // If we only have a JWT, do NOT persist the provider entry — it would be
  // stale on the next startup.  Configure it in-memory only so this session
  // works, but return false so callers know it wasn't persisted.
  const shouldPersist = !cred.isJwt;

  // Get available models from the Gateway API
  let models: string[];
  try {
    const res = await fetch(`${GATEWAY_URL}/models`, {
      headers: { Authorization: `Bearer ${cred.key}` },
    });
    if (res.ok) {
      const data = (await res.json()) as { data?: { id: string }[] };
      models = (data.data || []).map((m) => m.id);
    } else {
      models = [];
    }
  } catch {
    models = [];
  }

  if (models.length === 0) {
    // Fallback to known Gateway models
    models = [
      "claude-sonnet-4-6",
      "claude-opus-4-6",
      "gpt-4o",
      "models/gemini-2.5-pro",
      "qwen-3-235b-a22b-instruct-2507",
    ];
  }

  config.providers[PROVIDER_NAME] = {
    baseUrl: GATEWAY_URL,
    api: "openai-completions",
    apiKey: cred.key,
    compat: {
      supportsDeveloperRole: false,
      supportsReasoningEffort: false,
    },
    models: models.map((id: string) => ({ id })),
  };

  if (shouldPersist) {
    // Write back with restricted permissions
    mkdirSync(piDir, { recursive: true, mode: 0o700 });
    writeFileSync(modelsPath, JSON.stringify(config, null, 2) + "\n", {
      mode: 0o600,
    });
  }

  return shouldPersist;
}
