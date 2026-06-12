import { describe, it, expect, afterEach } from "vitest";
import { existsSync, readFileSync, mkdirSync, mkdtempSync, writeFileSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  ensureGatewayProvider,
  gatewayApiKeyRef,
  buildGatewayConfig,
} from "../src/gateway.js";

// ──────────────────────────────────────────────────────────
// gatewayApiKeyRef — pure: the request-time apiKey reference
// ──────────────────────────────────────────────────────────
// We always write a "!<mage> auth token" command reference. The mage CLI owns
// the full credential chain (JWT → refresh → vault → MAGELAB_API_KEY env), so a
// single reference is correct regardless of where Pi is launched from. No
// literal secret is ever written to models.json.

describe("gatewayApiKeyRef", () => {
  it("builds a `!<mage> auth token` command from the resolved binary path", () => {
    expect(gatewayApiKeyRef("/Users/x/.cargo/bin/mage")).toBe(
      "!/Users/x/.cargo/bin/mage auth token"
    );
  });

  it("leaves a shell-safe path unquoted", () => {
    expect(gatewayApiKeyRef("/usr/local/bin/mage")).toBe(
      "!/usr/local/bin/mage auth token"
    );
  });

  it("single-quotes a path containing spaces", () => {
    expect(gatewayApiKeyRef("/Users/Jane Doe/.cargo/bin/mage")).toBe(
      "!'/Users/Jane Doe/.cargo/bin/mage' auth token"
    );
  });

  it("never emits a literal secret or a $-interpolation", () => {
    const ref = gatewayApiKeyRef("/usr/local/bin/mage");
    expect(ref.startsWith("!")).toBe(true);
    expect(ref).not.toContain("$");
  });
});

// ──────────────────────────────────────────────────────────
// buildGatewayConfig — pure: merges the magelab provider
// ──────────────────────────────────────────────────────────

describe("buildGatewayConfig", () => {
  it("writes the reference apiKey, never a literal secret", () => {
    const config = buildGatewayConfig({ providers: {} }, "!mage auth token", [
      "claude-sonnet-4-6",
    ]);
    expect(config.providers.magelab.apiKey).toBe("!mage auth token");
    expect(config.providers.magelab.baseUrl).toBe("https://api.magelab.ai/v1");
    expect(config.providers.magelab.api).toBe("openai-completions");
    expect(config.providers.magelab.models).toEqual([
      { id: "claude-sonnet-4-6" },
    ]);
  });

  it("migrates a previously-embedded literal key to the reference form", () => {
    const existing = {
      providers: {
        magelab: {
          baseUrl: "https://api.magelab.ai/v1",
          apiKey: "mage_oldStaticKeyAtRest",
          models: [{ id: "stale" }],
        },
      },
    };
    const config = buildGatewayConfig(existing, "!mage auth token", ["m"]);
    expect(config.providers.magelab.apiKey).toBe("!mage auth token");
  });

  it("preserves other providers untouched", () => {
    const existing = {
      providers: {
        ollama: { baseUrl: "http://localhost:11434/v1", models: [{ id: "llama3" }] },
      },
    };
    const config = buildGatewayConfig(existing, "!mage auth token", ["m"]);
    expect(config.providers.ollama).toEqual({
      baseUrl: "http://localhost:11434/v1",
      models: [{ id: "llama3" }],
    });
    expect(config.providers.magelab).toBeDefined();
  });

  it("tolerates a missing providers object", () => {
    const config = buildGatewayConfig({}, "!mage auth token", ["m"]);
    expect(config.providers.magelab).toBeDefined();
  });

  it("does not mutate the input object", () => {
    const existing = { providers: { ollama: { models: [] } } };
    const snapshot = JSON.stringify(existing);
    buildGatewayConfig(existing, "!mage auth token", ["m"]);
    expect(JSON.stringify(existing)).toBe(snapshot);
  });
});

// ──────────────────────────────────────────────────────────
// ensureGatewayProvider — hermetic: all IO injected
// ──────────────────────────────────────────────────────────
// Dependencies (home, mageBin, getToken, fetchImpl) are injected so the test
// never touches the real ~/.pi, the real `mage` CLI, or the network.

let tempHome: string;

function makeTempHome() {
  // mkdtempSync guarantees a unique dir even under parallel workers / same-ms calls.
  tempHome = mkdtempSync(join(tmpdir(), "magelab-test-"));
  return tempHome;
}

const noNetwork: typeof fetch = async () => {
  throw new Error("network access not allowed in tests");
};

afterEach(() => {
  if (tempHome && existsSync(tempHome)) {
    rmSync(tempHome, { recursive: true, force: true });
  }
});

function modelsPathFor(home: string) {
  return join(home, ".pi", "agent", "models.json");
}

describe("ensureGatewayProvider", () => {
  it("writes a `!`-command reference even with no credentials, never the network", async () => {
    const home = makeTempHome();
    let fetchCalls = 0;
    const spyNoNetwork: typeof fetch = async (...args) => {
      fetchCalls++;
      return noNetwork(...args);
    };
    const result = await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => undefined,
      fetchImpl: spyNoNetwork,
    });
    expect(result).toBe(true);
    expect(fetchCalls).toBe(0); // no token → no network call attempted

    const config = JSON.parse(readFileSync(modelsPathFor(home), "utf-8"));
    const key = config.providers.magelab.apiKey as string;
    expect(key).toBe("!/usr/local/bin/mage auth token");
    expect(config.providers.magelab.models.length).toBeGreaterThan(0);
  });

  it("never persists a literal token, even when one is available", async () => {
    const home = makeTempHome();
    await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => "mage_secretLiteral",
      fetchImpl: async () =>
        new Response(JSON.stringify({ data: [{ id: "claude-sonnet-4-6" }] }), {
          status: 200,
        }),
    });
    const raw = readFileSync(modelsPathFor(home), "utf-8");
    expect(raw).not.toContain("mage_secretLiteral");
    expect(JSON.parse(raw).providers.magelab.apiKey).toBe(
      "!/usr/local/bin/mage auth token"
    );
  });

  it("populates models from the gateway when a token is available", async () => {
    const home = makeTempHome();
    let calledWith: { url: string; auth: string | null } | null = null;
    await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => "tok123",
      fetchImpl: async (input, init) => {
        calledWith = {
          url: String(input),
          auth: (init?.headers as Record<string, string>)?.Authorization ?? null,
        };
        return new Response(JSON.stringify({ data: [{ id: "m-1" }, { id: "m-2" }] }), {
          status: 200,
        });
      },
    });
    expect(calledWith!.url).toBe("https://api.magelab.ai/v1/models");
    expect(calledWith!.auth).toBe("Bearer tok123");
    const config = JSON.parse(readFileSync(modelsPathFor(home), "utf-8"));
    expect(config.providers.magelab.models).toEqual([{ id: "m-1" }, { id: "m-2" }]);
  });

  it("migrates an existing literal API key to the reference form", async () => {
    const home = makeTempHome();
    const piDir = join(home, ".pi", "agent");
    mkdirSync(piDir, { recursive: true });
    writeFileSync(
      join(piDir, "models.json"),
      JSON.stringify({
        providers: {
          magelab: {
            baseUrl: "https://api.magelab.ai/v1",
            apiKey: "mage_oldLiteralKey",
            models: [{ id: "test-model" }],
          },
        },
      })
    );

    await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => undefined,
      fetchImpl: noNetwork,
    });

    const raw = readFileSync(modelsPathFor(home), "utf-8");
    expect(raw).not.toContain("mage_oldLiteralKey");
    expect(JSON.parse(raw).providers.magelab.apiKey).toBe(
      "!/usr/local/bin/mage auth token"
    );
  });

  it("migrates an existing literal JWT to the reference form", async () => {
    const home = makeTempHome();
    const piDir = join(home, ".pi", "agent");
    mkdirSync(piDir, { recursive: true });
    // A realistic 3-segment JWT that the old code persisted on the broken path.
    const jwt =
      "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9." +
      "eyJzdWIiOiJ1c2VyXzEyMyIsImV4cCI6OTk5OTk5OTk5OSwiaXNzIjoid29ya29zIn0." +
      "c2lnbmF0dXJlLXBsYWNlaG9sZGVyLXZhbHVlLXRoYXQtaXMtbG9uZy1lbm91Z2g";
    writeFileSync(
      join(piDir, "models.json"),
      JSON.stringify({
        providers: {
          magelab: { baseUrl: "https://api.magelab.ai/v1", apiKey: jwt, models: [] },
        },
      })
    );

    await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => undefined,
      fetchImpl: noNetwork,
    });

    const raw = readFileSync(modelsPathFor(home), "utf-8");
    expect(raw).not.toContain(jwt);
    expect(JSON.parse(raw).providers.magelab.apiKey).toBe(
      "!/usr/local/bin/mage auth token"
    );
  });

  it("preserves existing providers when adding magelab", async () => {
    const home = makeTempHome();
    const piDir = join(home, ".pi", "agent");
    mkdirSync(piDir, { recursive: true });
    writeFileSync(
      join(piDir, "models.json"),
      JSON.stringify({
        providers: {
          ollama: { baseUrl: "http://localhost:11434/v1", models: [{ id: "llama3" }] },
        },
      })
    );

    await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => undefined,
      fetchImpl: noNetwork,
    });

    const config = JSON.parse(readFileSync(modelsPathFor(home), "utf-8"));
    expect(config.providers.ollama).toBeDefined();
    expect(config.providers.magelab).toBeDefined();
  });

  it("writes models.json with 0600 permissions", async () => {
    const home = makeTempHome();
    await ensureGatewayProvider({
      home,
      mageBin: "/usr/local/bin/mage",
      getToken: async () => undefined,
      fetchImpl: noNetwork,
    });
    if (process.platform !== "win32") {
      const mode = statSync(modelsPathFor(home)).mode & 0o777;
      expect(mode).toBe(0o600);
    }
  });
});
