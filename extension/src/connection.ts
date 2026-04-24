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

const CONNECT_ARGS = ["connect", "--json", "--no-launch"];

/** Find the magelab binary: PATH first, then ~/.cargo/bin/ */
function findMagelabBinary(): string {
  const cargoPath = join(homedir(), ".cargo", "bin", "magelab");
  if (existsSync(cargoPath)) return cargoPath;
  return "magelab"; // hope it's on PATH
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
    return JSON.parse(stdout) as ConnectionInfo;
  } catch (err: any) {
    if (err.code === "ENOENT") {
      throw new Error(
        "magelab CLI not found. Install: cargo install --path /path/to/magelab-cli"
      );
    }
    const stderr = err.stderr?.trim() || err.message;
    throw new Error(`magelab connect failed: ${stderr}`);
  }
}
