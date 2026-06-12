import { describe, it, expect, afterEach } from "vitest";
import { join } from "node:path";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  chmodSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { findMageBinary, configPaths, runMage } from "../src/binary.js";

// Helper: build cargo bin paths the same way the implementation does
const cargo = (home: string, name: string) => join(home, ".cargo", "bin", name);

// ──────────────────────────────────────────────────────────
// Tests for EXISTING behavior (pinned before any refactor)
// ──────────────────────────────────────────────────────────
// All three original call sites (connection.ts, gateway.ts,
// index.ts) had identical logic:
//   1. Check existsSync(join(homedir(), ".cargo", "bin", "magelab"))
//   2. If exists → return that full path
//   3. Else → return bare "magelab"
//
// The new module MUST still find legacy "magelab" so existing
// installs keep working.

describe("legacy compat: finds magelab at ~/.cargo/bin/magelab", () => {
  it("returns full cargo path when magelab exists", () => {
    const expected = cargo("/home/user", "magelab");
    const result = findMageBinary({
      home: "/home/user",
      platform: "linux",
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("works on macOS home paths", () => {
    const expected = cargo("/Users/alice", "magelab");
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "darwin",
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });
});

// ──────────────────────────────────────────────────────────
// New behavior: prefer canonical `mage` over legacy `magelab`
// ──────────────────────────────────────────────────────────
// Cargo.toml: [[bin]] name = "mage"
// A fresh `cargo install` produces `mage`, not `magelab`.

describe("mage-first: new installs use the canonical name", () => {
  it("returns ~/.cargo/bin/mage when it exists", () => {
    const expected = cargo("/Users/alice", "mage");
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "darwin",
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("prefers mage over magelab when both exist", () => {
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "darwin",
      exists: () => true,
    });
    expect(result).toBe(cargo("/Users/alice", "mage"));
  });

  it("falls back to magelab when mage absent", () => {
    const magelabPath = cargo("/Users/alice", "magelab");
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "darwin",
      exists: (p) => p === magelabPath,
    });
    expect(result).toBe(magelabPath);
  });

  it("returns bare 'mage' when neither cargo path exists", () => {
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "linux",
      exists: () => false,
    });
    expect(result).toBe("mage");
  });

  it("checks mage before magelab", () => {
    const checked: string[] = [];
    findMageBinary({
      home: "/home/user",
      platform: "linux",
      env: { PATH: "" },
      exists: (p) => { checked.push(p); return false; },
    });
    expect(checked).toEqual([
      cargo("/home/user", "mage"),
      cargo("/home/user", "magelab"),
    ]);
  });
});

// ──────────────────────────────────────────────────────────
// Windows: Gabe's command_path recipe (setup_pi.rs)
// ──────────────────────────────────────────────────────────
// On Windows cargo installs `mage.exe`. Node's execFile does
// NOT auto-resolve .exe/.cmd/.bat shims. Must check:
//   1. cargo bin with .exe extension
//   2. Walk PATH dirs × PATHEXT extensions
//
// Uses forward-slash home dirs so path.join works on macOS
// test runners (join uses host-OS separators).

describe("Windows: command_path recipe from setup_pi.rs", () => {
  const home = "C:/Users/dev";

  it("finds mage.exe in cargo bin", () => {
    const expected = cargo(home, "mage.exe");
    const result = findMageBinary({
      home,
      platform: "win32",
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("finds legacy magelab.exe when mage.exe absent", () => {
    const expected = cargo(home, "magelab.exe");
    const result = findMageBinary({
      home,
      platform: "win32",
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("walks PATH×PATHEXT to find mage.cmd shim", () => {
    const expected = join("C:/tools", "mage.cmd");
    const result = findMageBinary({
      home,
      platform: "win32",
      env: { PATH: "C:/tools", PATHEXT: ".EXE;.CMD" },
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("finds legacy magelab.cmd on PATH when mage not found", () => {
    const expected = join("C:/tools", "magelab.cmd");
    const result = findMageBinary({
      home,
      platform: "win32",
      env: { PATH: "C:/tools", PATHEXT: ".EXE;.CMD" },
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("uses default PATHEXT when not in env", () => {
    const checked: string[] = [];
    findMageBinary({
      home,
      platform: "win32",
      env: { PATH: "C:/bin" },
      exists: (p) => { checked.push(p); return false; },
    });
    const inBin = checked.filter((p) => p.startsWith(join("C:/bin", "")));
    expect(inBin).toContain(join("C:/bin", "mage.com"));
    expect(inBin).toContain(join("C:/bin", "mage.exe"));
    expect(inBin).toContain(join("C:/bin", "mage.bat"));
    expect(inBin).toContain(join("C:/bin", "mage.cmd"));
  });

  it("checks cargo .exe before PATH walk", () => {
    const checked: string[] = [];
    findMageBinary({
      home,
      platform: "win32",
      env: { PATH: "C:/bin", PATHEXT: ".EXE;.CMD" },
      exists: (p) => { checked.push(p); return false; },
    });
    expect(checked).toEqual([
      cargo(home, "mage.exe"),
      cargo(home, "magelab.exe"),
      join("C:/bin", "mage.exe"),
      join("C:/bin", "mage.cmd"),
      join("C:/bin", "magelab.exe"),
      join("C:/bin", "magelab.cmd"),
    ]);
  });

  it("returns bare 'mage' when nothing found", () => {
    const result = findMageBinary({
      home,
      platform: "win32",
      env: { PATH: "C:/tools", PATHEXT: ".EXE" },
      exists: () => false,
    });
    expect(result).toBe("mage");
  });
});

// ──────────────────────────────────────────────────────────
// Edge cases
// ──────────────────────────────────────────────────────────

describe("edge cases", () => {
  it("handles undefined env gracefully on Windows", () => {
    const result = findMageBinary({
      home: "C:/Users/dev",
      platform: "win32",
      env: undefined,
      exists: () => false,
    });
    expect(result).toBe("mage");
  });

  it("handles empty PATH string on Windows", () => {
    const result = findMageBinary({
      home: "C:/Users/dev",
      platform: "win32",
      env: { PATH: "", PATHEXT: ".EXE" },
      exists: () => false,
    });
    expect(result).toBe("mage");
  });

  it("handles empty PATHEXT string on Windows", () => {
    const checked: string[] = [];
    findMageBinary({
      home: "C:/Users/dev",
      platform: "win32",
      env: { PATH: "C:/bin", PATHEXT: "" },
      exists: (p) => { checked.push(p); return false; },
    });
    // Should still check cargo bin (.exe), but no PATH walk (no extensions)
    const pathChecks = checked.filter((p) => p.startsWith(join("C:/bin", "")));
    expect(pathChecks).toEqual([]);
  });

  it("first match wins across multiple PATH dirs", () => {
    const first = join("C:/first", "mage.exe");
    const second = join("C:/second", "mage.exe");
    const result = findMageBinary({
      home: "C:/Users/dev",
      platform: "win32",
      env: { PATH: "C:/first;C:/second", PATHEXT: ".EXE" },
      exists: (p) => p === first || p === second,
    });
    expect(result).toBe(first);
  });

  it("walks PATH on non-Windows platforms after cargo bin", () => {
    const checked: string[] = [];
    findMageBinary({
      home: "/home/user",
      platform: "linux",
      env: { PATH: "/usr/local/bin:/usr/bin", PATHEXT: ".EXE" },
      exists: (p) => { checked.push(p); return false; },
    });
    // Cargo bin first (both names), then PATH dirs (mage before magelab).
    expect(checked).toEqual([
      cargo("/home/user", "mage"),
      cargo("/home/user", "magelab"),
      join("/usr/local/bin", "mage"),
      join("/usr/bin", "mage"),
      join("/usr/local/bin", "magelab"),
      join("/usr/bin", "magelab"),
    ]);
  });

  it("finds a Homebrew-style mage on PATH when cargo bin is empty", () => {
    const expected = join("/opt/homebrew/bin", "mage");
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "darwin",
      env: { PATH: "/opt/homebrew/bin:/usr/bin" },
      exists: (p) => p === expected,
    });
    expect(result).toBe(expected);
  });

  it("prefers cargo mage over a PATH mage on POSIX", () => {
    const cargoMage = cargo("/Users/alice", "mage");
    const result = findMageBinary({
      home: "/Users/alice",
      platform: "darwin",
      env: { PATH: "/opt/homebrew/bin" },
      exists: () => true,
    });
    expect(result).toBe(cargoMage);
  });

  it("handles an empty PATH on POSIX (cargo only, then bare)", () => {
    const checked: string[] = [];
    const result = findMageBinary({
      home: "/home/user",
      platform: "linux",
      env: { PATH: "" },
      exists: (p) => { checked.push(p); return false; },
    });
    expect(checked).toEqual([
      cargo("/home/user", "mage"),
      cargo("/home/user", "magelab"),
    ]);
    expect(result).toBe("mage");
  });
});

// ──────────────────────────────────────────────────────────
// configPaths
// ──────────────────────────────────────────────────────────

describe("configPaths", () => {
  it("returns macOS and XDG paths", () => {
    const paths = configPaths("/Users/alice");
    expect(paths).toHaveLength(2);
    expect(paths[0]).toContain("Library");
    expect(paths[0]).toContain("cli.toml");
    expect(paths[1]).toContain(".config");
    expect(paths[1]).toContain("cli.toml");
  });
});

// ──────────────────────────────────────────────────────────
// Production default: real isExecutableFile (no injected `exists`)
// ──────────────────────────────────────────────────────────
// These exercise the actual default predicate against a real temp filesystem,
// which the injected-`exists` tests above deliberately bypass.

describe("findMageBinary (real filesystem default)", () => {
  let dir: string;
  afterEach(() => {
    if (dir) rmSync(dir, { recursive: true, force: true });
  });

  it("finds a real executable `mage` on PATH", () => {
    dir = mkdtempSync(join(tmpdir(), "magebin-"));
    const exe = join(dir, "mage");
    writeFileSync(exe, "#!/bin/sh\necho hi\n");
    chmodSync(exe, 0o755);
    // home points nowhere, so resolution falls through to the PATH walk.
    const result = findMageBinary({
      home: join(dir, "no-home"),
      platform: process.platform,
      env: { PATH: dir },
    });
    expect(result).toBe(exe);
  });

  it("skips a non-executable file named `mage` (POSIX)", () => {
    if (process.platform === "win32") return; // X_OK is existence-only on Windows
    dir = mkdtempSync(join(tmpdir(), "magebin-"));
    const f = join(dir, "mage");
    writeFileSync(f, "not a program");
    chmodSync(f, 0o644);
    const result = findMageBinary({
      home: join(dir, "no-home"),
      platform: "linux",
      env: { PATH: dir },
    });
    expect(result).toBe("mage"); // fell through, not the bogus file
  });

  it("skips a directory named `mage`", () => {
    if (process.platform === "win32") return;
    dir = mkdtempSync(join(tmpdir(), "magebin-"));
    mkdirSync(join(dir, "mage"));
    const result = findMageBinary({
      home: join(dir, "no-home"),
      platform: "linux",
      env: { PATH: dir },
    });
    expect(result).toBe("mage");
  });
});

// ──────────────────────────────────────────────────────────
// runMage — exec wiring (injected exec)
// ──────────────────────────────────────────────────────────

describe("runMage", () => {
  it("passes args + timeout + maxBuffer to exec and returns stdout", async () => {
    let captured: { bin: string; args: string[]; opts: any } | null = null;
    const fakeExec = (async (bin: string, args: string[], opts: any) => {
      captured = { bin, args, opts };
      return { stdout: "OUTPUT", stderr: "" };
    }) as any;

    const out = await runMage(["balance"], { bin: "/x/mage", exec: fakeExec });

    expect(out).toBe("OUTPUT");
    expect(captured!.bin).toBe("/x/mage");
    expect(captured!.args).toEqual(["balance"]);
    expect(captured!.opts).toMatchObject({ timeout: 15000, maxBuffer: 1024 * 1024 });
  });

  it("honors a custom timeout", async () => {
    let opts: any = null;
    const fakeExec = (async (_b: string, _a: string[], o: any) => {
      opts = o;
      return { stdout: "", stderr: "" };
    }) as any;
    await runMage(["version"], { bin: "/x/mage", exec: fakeExec, timeoutMs: 500 });
    expect(opts.timeout).toBe(500);
  });
});
