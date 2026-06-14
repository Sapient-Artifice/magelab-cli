/**
 * Shared constants and utilities for locating and invoking MageLab CLI artifacts.
 */
import { execFile } from "node:child_process";
import { accessSync, constants as fsConstants, statSync } from "node:fs";
import { join } from "node:path";
import { homedir, platform as osPlatform } from "node:os";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/**
 * True only for an existing, regular (or symlink-to-regular), *executable*
 * file. Plain existence is not enough: a directory or a non-executable file
 * literally named `mage` on PATH would otherwise be "found" and then fail
 * execFile with EISDIR/EACCES instead of ENOENT, misclassifying the error.
 */
function isExecutableFile(p: string): boolean {
  try {
    if (!statSync(p).isFile()) return false;
    accessSync(p, fsConstants.X_OK);
    return true;
  } catch {
    return false;
  }
}

// ── CLI config paths (used by index.ts) ──

/** Returns the platform-specific candidate paths for cli.toml */
export function configPaths(home: string): string[] {
  return [
    // macOS
    join(home, "Library", "Application Support", "magelab", "cli.toml"),
    // Linux / Windows (XDG / fallback)
    join(home, ".config", "magelab", "cli.toml"),
  ];
}

// ── Binary resolution ──

const NAMES = ["mage", "magelab"] as const;
const DEFAULT_PATHEXT = ".COM;.EXE;.BAT;.CMD";

export interface FindBinaryOpts {
  home: string;
  platform: string;
  exists: (path: string) => boolean;
  env?: { PATH?: string; PATHEXT?: string };
}

function defaults(): FindBinaryOpts {
  return {
    home: homedir(),
    platform: osPlatform(),
    exists: isExecutableFile,
    env: { PATH: process.env.PATH, PATHEXT: process.env.PATHEXT },
  };
}

/**
 * Resolve the MageLab CLI binary to an absolute path.
 *
 * Search order: ~/.cargo/bin, then every directory on PATH. `mage` (the
 * canonical name) is preferred over the legacy `magelab` at each step. Walking
 * PATH on POSIX too (not just Windows) is important: many users install via
 * Homebrew (/opt/homebrew/bin) or to /usr/local/bin, and Pi resolves the
 * "!<path> auth token" reference with whatever PATH it has at request time —
 * which, for GUI-launched terminals, often excludes ~/.cargo/bin. Returning an
 * absolute path makes that reference robust. Falls back to the bare name only
 * when nothing is found.
 */
export function findMageBinary(opts?: Partial<FindBinaryOpts>): string {
  const { home, platform, exists, env } = { ...defaults(), ...opts };
  const win = platform === "win32";

  // 1. Cargo bin (with .exe on Windows).
  for (const name of NAMES) {
    const file = win ? `${name}.exe` : name;
    const p = join(home, ".cargo", "bin", file);
    if (exists(p)) return p;
  }

  // 2. Walk PATH × extensions (both platforms).
  const sep = win ? ";" : ":";
  const dirs = (env?.PATH ?? "").split(sep).filter(Boolean);
  const exts = win
    ? (env?.PATHEXT ?? DEFAULT_PATHEXT)
        .split(";")
        .filter(Boolean)
        .map((e) => e.toLowerCase())
    : [""];
  for (const name of NAMES) {
    for (const dir of dirs) {
      for (const ext of exts) {
        const p = join(dir, `${name}${ext}`);
        if (exists(p)) return p;
      }
    }
  }

  return "mage";
}

// ── Invocation ──

export interface RunMageOpts {
  /** Override the resolved binary (default: findMageBinary()). */
  bin?: string;
  /** Override the exec function (for tests). */
  exec?: typeof execFileAsync;
  /** Kill the subprocess after this many ms (default 15000). */
  timeoutMs?: number;
}

/**
 * Run the MageLab CLI with the given args and return stdout. Throws the raw
 * execFile error (with `.code` set, e.g. "ENOENT") so callers can branch on it.
 * Centralizes the binary lookup + exec pattern; deps are injectable for tests.
 * A timeout + bounded buffer guard against a hung or runaway subprocess.
 */
export async function runMage(args: string[], opts: RunMageOpts = {}): Promise<string> {
  const bin = opts.bin ?? findMageBinary();
  const exec = opts.exec ?? execFileAsync;
  const { stdout } = await exec(bin, args, {
    timeout: opts.timeoutMs ?? 15_000,
    maxBuffer: 1024 * 1024,
  });
  return stdout as string;
}
