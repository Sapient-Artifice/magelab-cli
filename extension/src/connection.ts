import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export interface ConnectionInfo {
  url: string | null;
  token: string | null;
  mode: "local" | "relay" | "remote" | "none";
  model: string | null;
}

const VALID_MODES = new Set(["local", "relay", "remote", "none"]);

const CONNECT_ARGS = ["connect", "--json", "--no-launch"];

/** Validate that parsed JSON matches the ConnectionInfo shape */
export function validateConnectionInfo(data: unknown): ConnectionInfo {
  if (!data || typeof data !== "object") {
    throw new Error("Invalid connection info: expected an object");
  }
  const obj = data as Record<string, unknown>;
  if (typeof obj.mode !== "string" || !VALID_MODES.has(obj.mode)) {
    throw new Error(`Invalid mode: "${obj.mode}" — expected one of: ${[...VALID_MODES].join(", ")}`);
  }
  if (obj.mode !== "none" && obj.url !== null && typeof obj.url !== "string") {
    throw new Error(`Invalid url: expected string or null, got ${typeof obj.url}`);
  }
  return obj as unknown as ConnectionInfo;
}

/** Find the mage binary: PATH first, then ~/.cargo/bin/ */
function findMagelabBinary(): string {
  const cargoPath = join(homedir(), ".cargo", "bin", "mage");
  if (existsSync(cargoPath)) return cargoPath;
  return "mage"; // hope it's on PATH
}

/**
 * Run `magelab connect --json --no-launch` and parse the result.
 * Looks for the binary on PATH and in ~/.cargo/bin/.
 * Throws if the binary is not found or returns non-zero.
 */
export async function getConnection(): Promise<ConnectionInfo> {
  const bin = findMagelabBinary();
  try {
    const { stdout } = await execFileAsync(bin, CONNECT_ARGS);
    return validateConnectionInfo(JSON.parse(stdout));
  } catch (err: any) {
    if (err.code === "ENOENT") {
      throw new Error(
        "mage CLI not found. Install: cargo install --path /path/to/magelab-cli"
      );
    }
    const stderr = err.stderr?.trim() || err.message;
    throw new Error(`mage connect failed: ${stderr}`);
  }
}
