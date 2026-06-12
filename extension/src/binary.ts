import { existsSync } from "node:fs";
import { delimiter, join } from "node:path";
import { homedir } from "node:os";

export interface BinaryLookupDeps {
  existsSync?: (path: string) => boolean;
  homedir?: () => string;
  env?: NodeJS.ProcessEnv;
  platform?: NodeJS.Platform;
  pathDelimiter?: string;
}

const BINARY_NAMES = ["mage", "magelab"];
const WINDOWS_PATHEXT = ".COM;.EXE;.BAT;.CMD";

function executableCandidates(name: string, deps: Required<BinaryLookupDeps>): string[] {
  if (deps.platform !== "win32") return [name];

  const hasExtension = /\.[^\\/]+$/.test(name);
  if (hasExtension) return [name];

  const extensions = (deps.env.PATHEXT || WINDOWS_PATHEXT)
    .split(";")
    .filter(Boolean);
  return extensions.map((ext) => `${name}${ext}`);
}

function defaultDeps(deps: BinaryLookupDeps = {}): Required<BinaryLookupDeps> {
  return {
    existsSync: deps.existsSync || existsSync,
    homedir: deps.homedir || homedir,
    env: deps.env || process.env,
    platform: deps.platform || process.platform,
    pathDelimiter: deps.pathDelimiter || delimiter,
  };
}

export function configPaths(home = homedir()): string[] {
  return [
    join(home, "Library", "Application Support", "magelab", "cli.toml"),
    join(home, ".config", "magelab", "cli.toml"),
  ];
}

export function findMageBinary(lookupDeps: BinaryLookupDeps = {}): string {
  const deps = defaultDeps(lookupDeps);
  const home = deps.homedir();

  for (const name of BINARY_NAMES) {
    for (const candidate of executableCandidates(name, deps)) {
      const cargoPath = join(home, ".cargo", "bin", candidate);
      if (deps.existsSync(cargoPath)) return cargoPath;
    }

    const pathValue = deps.env.PATH || "";
    for (const dir of pathValue.split(deps.pathDelimiter).filter(Boolean)) {
      for (const candidate of executableCandidates(name, deps)) {
        const pathCandidate = join(dir, candidate);
        if (deps.existsSync(pathCandidate)) return pathCandidate;
      }
    }
  }

  return "mage";
}
