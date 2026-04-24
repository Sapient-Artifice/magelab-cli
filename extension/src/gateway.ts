/**
 * Configure Pi to use MageLab Gateway as an LLM provider.
 *
 * Writes ~/.pi/agent/models.json with the Gateway as an OpenAI-compatible
 * provider, using `magelab auth token` for authentication.
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
 * Get the best available API key: env var > CLI config > auth token.
 */
async function getApiKey(): Promise<string> {
  // 1. Environment variable
  const envKey = process.env.MAGELAB_API_KEY;
  if (envKey) return envKey;

  // 2. Read from CLI config file
  const configPath = join(homedir(), "Library", "Application Support", "magelab", "cli.toml");
  if (existsSync(configPath)) {
    const content = readFileSync(configPath, "utf-8");
    const match = content.match(/api_key\s*=\s*"([^"]+)"/);
    if (match) return match[1];
  }

  // 3. Also check XDG config
  const xdgPath = join(homedir(), ".config", "magelab", "cli.toml");
  if (existsSync(xdgPath)) {
    const content = readFileSync(xdgPath, "utf-8");
    const match = content.match(/api_key\s*=\s*"([^"]+)"/);
    if (match) return match[1];
  }

  // 4. Try magelab auth token
  const bin = findBinary();
  const { stdout } = await execFileAsync(bin, ["auth", "token"]);
  const token = stdout.trim();
  if (token) return token;

  throw new Error("No API key found");
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

  // Skip if already configured
  if (config.providers[PROVIDER_NAME]) return true;

  // Get available models from the Gateway API
  let models: string[];
  try {
    const token = await getApiKey();
    const res = await fetch(`${GATEWAY_URL}/models`, {
      headers: { Authorization: `Bearer ${token}` },
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

  // Get the API key for the provider config
  let apiKey: string;
  try {
    apiKey = await getApiKey();
  } catch {
    return false; // Can't configure without a key
  }

  config.providers[PROVIDER_NAME] = {
    baseUrl: GATEWAY_URL,
    api: "openai-completions",
    apiKey,
    compat: {
      supportsDeveloperRole: false,
      supportsReasoningEffort: false,
    },
    models: models.map((id: string) => ({ id })),
  };

  // Write back
  mkdirSync(piDir, { recursive: true });
  writeFileSync(modelsPath, JSON.stringify(config, null, 2) + "\n", {
    mode: 0o600,
  });

  return true;
}
