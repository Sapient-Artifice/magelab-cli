import { describe, it, expect, afterEach } from "vitest";
import { existsSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { getConnection, validateConnectionInfo } from "../src/connection.js";

// These tests exercise getConnection against the real `mage`/`magelab` binary.
// If neither is installed, they verify the ENOENT fallback behavior.

const hasMageCli = (() => {
  try {
    const { execFileSync } = require("node:child_process");
    execFileSync("mage", ["version"], { stdio: "ignore" });
    return true;
  } catch {
    // Also check legacy magelab and cargo-installed binaries.
    const cargoMagePath = join(require("node:os").homedir(), ".cargo", "bin", "mage");
    const cargoMagelabPath = join(require("node:os").homedir(), ".cargo", "bin", "magelab");
    try {
      const { execFileSync } = require("node:child_process");
      execFileSync("magelab", ["version"], { stdio: "ignore" });
      return true;
    } catch {
      try {
        const { execFileSync } = require("node:child_process");
        execFileSync(cargoMagePath, ["version"], { stdio: "ignore" });
        return true;
      } catch {
        try {
          const { execFileSync } = require("node:child_process");
          execFileSync(cargoMagelabPath, ["version"], { stdio: "ignore" });
          return true;
        } catch {
          return false;
        }
      }
    }
  }
})();

describe("getConnection", () => {
  it("returns a ConnectionInfo object with expected fields", async () => {
    if (!hasMageCli) {
      // Without mage/magelab, getConnection should throw with install instructions
      await expect(getConnection()).rejects.toThrow("mage");
      return;
    }

    let conn;
    try {
      conn = await getConnection();
    } catch (err: any) {
      expect(err.message).toContain("mage connect failed");
      return;
    }
    expect(conn).toHaveProperty("url");
    expect(conn).toHaveProperty("token");
    expect(conn).toHaveProperty("mode");
    expect(conn).toHaveProperty("model");
    expect(["local", "relay", "remote", "none"]).toContain(conn.mode);
  });

  it("returns mode with correct type", async () => {
    if (!hasMageCli) return;

    const conn = await getConnection();
    expect(typeof conn.mode).toBe("string");
    if (conn.url !== null) {
      expect(typeof conn.url).toBe("string");
    }
  });
});

describe("validateConnectionInfo", () => {
  it("accepts valid connection info", () => {
    expect(() =>
      validateConnectionInfo({ url: "ws://localhost/ws", token: null, mode: "local", model: "gpt-4" })
    ).not.toThrow();
  });

  it("accepts mode=none with null url", () => {
    expect(() =>
      validateConnectionInfo({ url: null, token: null, mode: "none", model: null })
    ).not.toThrow();
  });

  it("rejects invalid mode", () => {
    expect(() =>
      validateConnectionInfo({ url: null, token: null, mode: "bogus" as any, model: null })
    ).toThrow("Invalid mode");
  });

  it("rejects missing mode field", () => {
    expect(() =>
      validateConnectionInfo({ url: null, token: null } as any)
    ).toThrow();
  });

  it("rejects non-string url when mode is not none", () => {
    expect(() =>
      validateConnectionInfo({ url: 42, token: null, mode: "local", model: null } as any)
    ).toThrow();
  });
});
