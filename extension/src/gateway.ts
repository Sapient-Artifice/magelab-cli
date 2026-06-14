/**
 * Configure Pi to use MageLab Gateway as an LLM provider.
 *
 * Writes ~/.pi/agent/models.json with the Gateway as an OpenAI-compatible
 * provider. The stored `apiKey` is always a request-time *command reference*:
 * "!<mage> auth token". Pi runs this per request (it resolves "!cmd" apiKeys at
 * request time, no caching — see earendil-works/pi coding-agent docs), and the
 * mage CLI owns the full credential chain (JWT → refresh → vault →
 * MAGELAB_API_KEY env). A single reference is therefore correct no matter where
 * Pi is launched from, and no literal secret is ever written to models.json.
 */
import { existsSync, readFileSync, writeFileSync, mkdirSync, chmodSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { findMageBinary, runMage } from "./binary.js";

const GATEWAY_URL = "https://api.magelab.ai/v1";
const PROVIDER_NAME = "magelab";

/** Models advertised when the live /models call is unavailable. */
const KNOWN_MODELS = [
  "claude-sonnet-4-6",
  "claude-opus-4-6",
  "gpt-4o",
  "models/gemini-2.5-pro",
  "qwen-3-235b-a22b-instruct-2507",
];

/**
 * Quote a path for use inside a Pi `"!cmd"` apiKey. Shell-safe paths are left
 * bare; anything else (e.g. a space in $HOME) is single-quoted. Pi executes
 * "!" references through a shell, so POSIX single-quoting applies.
 *
 * KNOWN LIMITATION (Windows): cmd.exe/PowerShell do not honor POSIX
 * single-quotes, so a Windows install path containing spaces
 * (e.g. C:\Users\Jane Doe\.cargo\bin\mage.exe) is not correctly quoted here.
 * Bare (space-free) Windows paths work. Tracked for a follow-up once Pi's
 * Windows "!cmd" execution semantics are confirmed.
 */
function shellArg(s: string): string {
  // Conservative bare-safe set. `%` and `,` are deliberately excluded: `%` is a
  // variable sigil under cmd.exe and would be left bare, so quote it instead.
  if (/^[A-Za-z0-9_@+=:./-]+$/.test(s)) return s;
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/**
 * Build the request-time apiKey reference Pi resolves: a "!<mage> auth token"
 * command. Never returns a literal secret.
 */
export function gatewayApiKeyRef(mageBin: string): string {
  return `!${shellArg(mageBin)} auth token`;
}

/**
 * Merge the MageLab Gateway provider into an existing models.json config.
 * Overwrites any prior `magelab` entry (migrating an embedded literal key/JWT
 * to the reference form) while preserving all other providers. Pure: does not
 * mutate `existing`.
 */
export function buildGatewayConfig(
  existing: any,
  apiKeyRef: string,
  models: string[]
): any {
  const base = existing && typeof existing === "object" ? existing : {};
  const config: any = { ...base };
  config.providers = { ...(base.providers || {}) };
  config.providers[PROVIDER_NAME] = {
    baseUrl: GATEWAY_URL,
    api: "openai-completions",
    apiKey: apiKeyRef,
    compat: {
      supportsDeveloperRole: false,
      supportsReasoningEffort: false,
    },
    models: models.map((id) => ({ id })),
  };
  return config;
}

/** Injectable dependencies for ensureGatewayProvider (real defaults below). */
export interface GatewayDeps {
  home: string;
  mageBin: string;
  /** Resolve a concrete token for the one-time model-list fetch; never persisted. */
  getToken: () => Promise<string | undefined>;
  fetchImpl: typeof fetch;
}

/** Resolve a concrete bearer token via the CLI's full credential chain. */
async function resolveTokenViaMage(): Promise<string | undefined> {
  try {
    // `--no-touchid`: this runs at every Pi startup purely to list models, so
    // it must never trigger a biometric prompt. It only fetches a read-only
    // model list and the token is not persisted.
    const out = await runMage(["--no-touchid", "auth", "token"]);
    const token = out.trim();
    return token || undefined;
  } catch {
    return undefined;
  }
}

function gatewayDefaults(): GatewayDeps {
  return {
    home: homedir(),
    mageBin: findMageBinary(),
    getToken: resolveTokenViaMage,
    fetchImpl: (input: any, init?: any) => fetch(input, init),
  };
}

/** Fetch the Gateway's model ids, falling back to the known list. */
async function fetchGatewayModels(
  token: string | undefined,
  fetchImpl: typeof fetch
): Promise<string[]> {
  if (token) {
    try {
      const res = await fetchImpl(`${GATEWAY_URL}/models`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (res.ok) {
        const data = (await res.json()) as { data?: { id: string }[] };
        const ids = (data.data || []).map((m) => m.id);
        if (ids.length > 0) return ids;
      }
    } catch {
      // Network/parse failure — fall through to the known list.
    }
  }
  // No live list available (logged out at startup, or /models unreachable). The
  // static fallback may drift from the gateway's real catalog, so leave a trace.
  console.warn(
    "[magelab] using built-in model list (gateway model fetch unavailable)"
  );
  return KNOWN_MODELS;
}

/**
 * Ensure ~/.pi/agent/models.json has the MageLab Gateway configured with a
 * request-time apiKey reference. Always (re)writes the `magelab` provider so a
 * stale literal credential is migrated to the reference form; other providers
 * are preserved. Returns true once written.
 */
export async function ensureGatewayProvider(
  opts?: Partial<GatewayDeps>
): Promise<boolean> {
  const { home, mageBin, getToken, fetchImpl } = { ...gatewayDefaults(), ...opts };

  const piDir = join(home, ".pi", "agent");
  const modelsPath = join(piDir, "models.json");

  // Load existing models.json or start fresh.
  let existing: any = { providers: {} };
  if (existsSync(modelsPath)) {
    try {
      existing = JSON.parse(readFileSync(modelsPath, "utf-8"));
    } catch {
      existing = { providers: {} };
    }
  }

  const apiKeyRef = gatewayApiKeyRef(mageBin);

  // Resolve a concrete token only to populate the model list; never persisted.
  const token = await getToken();
  const models = await fetchGatewayModels(token, fetchImpl);

  const config = buildGatewayConfig(existing, apiKeyRef, models);

  mkdirSync(piDir, { recursive: true, mode: 0o700 });
  writeFileSync(modelsPath, JSON.stringify(config, null, 2) + "\n", {
    mode: 0o600,
  });
  // writeFileSync `mode` is masked by umask and skipped if the file already
  // exists, so tighten the file explicitly. We do NOT chmod piDir — Pi owns
  // ~/.pi/agent and may store other config there; forcing its perms would be a
  // surprising side effect. models.json itself never contains a secret anyway.
  if (process.platform !== "win32") {
    try {
      chmodSync(modelsPath, 0o600);
    } catch {
      // Best-effort on exotic filesystems.
    }
  }

  return true;
}
