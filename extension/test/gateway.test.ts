import { describe, it, expect, afterEach } from "vitest";
import { existsSync, readFileSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { ensureGatewayProvider } from "../src/gateway.js";

// Gateway tests use a temp HOME to avoid touching real ~/.pi/agent/models.json

let tempHome: string;

function setTempHome() {
  tempHome = join(tmpdir(), `magelab-test-${Date.now()}`);
  mkdirSync(tempHome, { recursive: true });
  // Override HOME for this process
  process.env.__ORIGINAL_HOME = process.env.HOME;
  process.env.HOME = tempHome;
}

afterEach(() => {
  if (process.env.__ORIGINAL_HOME) {
    process.env.HOME = process.env.__ORIGINAL_HOME;
    delete process.env.__ORIGINAL_HOME;
  }
  if (tempHome && existsSync(tempHome)) {
    rmSync(tempHome, { recursive: true, force: true });
  }
});

describe("ensureGatewayProvider", () => {
  it("skips if magelab provider already configured", async () => {
    setTempHome();
    const piDir = join(tempHome, ".pi", "agent");
    mkdirSync(piDir, { recursive: true });
    writeFileSync(
      join(piDir, "models.json"),
      JSON.stringify({
        providers: {
          magelab: {
            baseUrl: "https://api.magelab.ai/v1",
            apiKey: "existing-key",
            models: [{ id: "test-model" }],
          },
        },
      })
    );

    const result = await ensureGatewayProvider();
    expect(result).toBe(true);

    // Should not have modified the file
    const config = JSON.parse(readFileSync(join(piDir, "models.json"), "utf-8"));
    expect(config.providers.magelab.apiKey).toBe("existing-key");
  });

  it("preserves existing providers when adding magelab", async () => {
    setTempHome();
    const piDir = join(tempHome, ".pi", "agent");
    mkdirSync(piDir, { recursive: true });
    writeFileSync(
      join(piDir, "models.json"),
      JSON.stringify({
        providers: {
          ollama: {
            baseUrl: "http://localhost:11434/v1",
            models: [{ id: "llama3" }],
          },
        },
      })
    );

    // This will fail to get an API key (no magelab in temp home), so it returns false
    const result = await ensureGatewayProvider();

    // Check ollama is still there
    if (existsSync(join(piDir, "models.json"))) {
      const config = JSON.parse(readFileSync(join(piDir, "models.json"), "utf-8"));
      expect(config.providers.ollama).toBeDefined();
    }
  });
});
