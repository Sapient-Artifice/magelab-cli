import { describe, expect, it } from "vitest";
import { join } from "node:path";
import { configPaths, findMageBinary, type BinaryLookupDeps } from "../src/binary.js";

function lookup(existing: string[], deps: BinaryLookupDeps = {}) {
  const files = new Set(existing);
  return findMageBinary({
    homedir: () => "/home/tester",
    env: {},
    pathDelimiter: ":",
    platform: "linux",
    ...deps,
    existsSync: (path) => files.has(path),
  });
}

describe("findMageBinary", () => {
  it("prefers mage in cargo bin over legacy magelab", () => {
    expect(
      lookup([
        "/home/tester/.cargo/bin/mage",
        "/home/tester/.cargo/bin/magelab",
      ])
    ).toBe("/home/tester/.cargo/bin/mage");
  });

  it("falls back to legacy magelab for existing installs", () => {
    expect(lookup(["/home/tester/.cargo/bin/magelab"])).toBe(
      "/home/tester/.cargo/bin/magelab"
    );
  });

  it("searches PATH before falling back to the bare mage command", () => {
    expect(
      lookup(["/opt/bin/mage"], {
        env: { PATH: "/usr/bin:/opt/bin" },
      })
    ).toBe("/opt/bin/mage");
  });

  it("returns bare mage when no candidate exists", () => {
    expect(lookup([])).toBe("mage");
  });

  it("resolves Windows PATHEXT shims on PATH", () => {
    expect(
      lookup([join("C:\\Users\\tester", ".cargo", "bin", "mage.CMD")], {
        homedir: () => "C:\\Users\\tester",
        env: { PATH: "C:\\npm;C:\\bin", PATHEXT: ".COM;.EXE;.BAT;.CMD" },
        pathDelimiter: ";",
        platform: "win32",
      })
    ).toBe(join("C:\\Users\\tester", ".cargo", "bin", "mage.CMD"));
  });

  it("checks mage before magelab on Windows PATH", () => {
    expect(
      lookup([join("C:\\npm", "mage.CMD"), join("C:\\npm", "magelab.CMD")], {
        homedir: () => "C:\\Users\\tester",
        env: { PATH: "C:\\npm", PATHEXT: ".CMD" },
        pathDelimiter: ";",
        platform: "win32",
      })
    ).toBe(join("C:\\npm", "mage.CMD"));
  });
});

describe("configPaths", () => {
  it("returns macOS and XDG config locations", () => {
    expect(configPaths("/home/tester")).toEqual([
      join("/home/tester", "Library", "Application Support", "magelab", "cli.toml"),
      join("/home/tester", ".config", "magelab", "cli.toml"),
    ]);
  });
});
