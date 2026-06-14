import { describe, it, expect } from "vitest";
import { getConnection, validateConnectionInfo } from "../src/connection.js";

// getConnection's logic is covered hermetically via an injected runner — no
// dependency on a real `mage` binary being installed (that was flaky/non-
// deterministic in CI).

describe("getConnection (injected runner — hermetic)", () => {
  const throwing = (code: string, extra: Record<string, unknown> = {}) => async () => {
    const e: any = new Error(`spawn ${code}`);
    e.code = code;
    Object.assign(e, extra);
    throw e;
  };
  const errorFrom = (run: (args: string[]) => Promise<string>) =>
    getConnection(run).then(
      () => {
        throw new Error("expected getConnection to reject");
      },
      (e: any) => e
    );

  it("parses valid stdout from the runner", async () => {
    const run = async () =>
      JSON.stringify({ url: null, token: null, mode: "none", model: null });
    const conn = await getConnection(run);
    expect(conn.mode).toBe("none");
  });

  it("normalizes ENOENT to a 'CLI not found' error tagged ENOENT", async () => {
    const e = await errorFrom(throwing("ENOENT"));
    expect(e.code).toBe("ENOENT");
    expect(e.message).toMatch(/not found/i);
  });

  // A present-but-not-executable `mage` fails with EACCES (not ENOENT). It must
  // still short-circuit retries (code ENOENT) but give a DIFFERENT, actionable
  // message: reinstalling won't fix a permission bit.
  it("treats EACCES as not-usable with a 'not executable' hint, tagged ENOENT", async () => {
    const e = await errorFrom(throwing("EACCES"));
    expect(e.code).toBe("ENOENT");
    expect(e.message).toMatch(/not executable/i);
    expect(e.message).not.toMatch(/setup-pi/i);
  });

  // A directory / bad path component named `mage` yields EISDIR/ENOTDIR — treat
  // as not-found (reinstall hint is appropriate there).
  for (const code of ["EISDIR", "ENOTDIR"]) {
    it(`treats ${code} as not-found (tagged ENOENT)`, async () => {
      const e = await errorFrom(throwing(code));
      expect(e.code).toBe("ENOENT");
      expect(e.message).toMatch(/not found/i);
    });
  }

  it("surfaces other errors as 'mage connect failed: <stderr>'", async () => {
    const e = await errorFrom(throwing("EOTHER", { stderr: "bad stuff" }));
    expect(e.message).toMatch(/mage connect failed: bad stuff/);
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
