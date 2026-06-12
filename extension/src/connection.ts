import { runMage } from "./binary.js";

export interface ConnectionInfo {
  url: string | null;
  token: string | null;
  mode: "local" | "relay" | "remote" | "none";
  model: string | null;
}

const VALID_MODES = new Set(["local", "relay", "remote", "none"]);

const CONNECT_ARGS = ["connect", "--json", "--no-launch"];

// execFile error codes that all mean "the mage binary isn't usable": missing
// (ENOENT), present-but-not-executable (EACCES), or a directory / bad path
// component (EISDIR/ENOTDIR). All are normalized to ENOENT so callers show the
// setup hint instead of retrying a confusing "connect failed".
const NOT_USABLE_CODES = new Set(["ENOENT", "EACCES", "EISDIR", "ENOTDIR"]);

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

/**
 * Run `mage connect --json --no-launch` and parse the result.
 * Throws if the binary is not found or returns non-zero.
 */
export async function getConnection(
  run: (args: string[]) => Promise<string> = runMage
): Promise<ConnectionInfo> {
  try {
    const stdout = await run(CONNECT_ARGS);
    return validateConnectionInfo(JSON.parse(stdout));
  } catch (err: any) {
    if (NOT_USABLE_CODES.has(err?.code)) {
      // EACCES = the binary exists but isn't executable; reinstalling won't fix
      // a permission bit, so give a distinct, actionable message. Still tag
      // ENOENT so the startup retry loop short-circuits.
      const message =
        err.code === "EACCES"
          ? "CLI found but not executable — run: chmod +x on the mage binary"
          : "CLI not found. Run: mage setup-pi";
      const e: any = new Error(message);
      e.code = "ENOENT";
      throw e;
    }
    const stderr = err.stderr?.trim() || err.message;
    throw new Error(`mage connect failed: ${stderr}`);
  }
}
