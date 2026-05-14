import { describe, it, expect } from "vitest";
import { isAllowedUrl, isAllowedFilepath } from "../src/validation.js";

describe("isAllowedUrl", () => {
  it("allows http URLs", () => {
    expect(isAllowedUrl("http://example.com")).toBe(true);
  });

  it("allows https URLs", () => {
    expect(isAllowedUrl("https://docs.magelab.ai")).toBe(true);
  });

  it("rejects file:// URLs", () => {
    expect(isAllowedUrl("file:///etc/passwd")).toBe(false);
  });

  it("rejects javascript: URLs", () => {
    expect(isAllowedUrl("javascript:alert(1)")).toBe(false);
  });

  it("rejects empty strings", () => {
    expect(isAllowedUrl("")).toBe(false);
  });

  it("rejects non-string input", () => {
    expect(isAllowedUrl(undefined as any)).toBe(false);
    expect(isAllowedUrl(42 as any)).toBe(false);
  });
});

describe("isAllowedFilepath", () => {
  it("allows absolute paths", () => {
    expect(isAllowedFilepath("/Users/test/project/README.md")).toBe(true);
  });

  it("rejects relative paths", () => {
    expect(isAllowedFilepath("../../../etc/passwd")).toBe(false);
  });

  it("rejects empty strings", () => {
    expect(isAllowedFilepath("")).toBe(false);
  });

  it("rejects non-string input", () => {
    expect(isAllowedFilepath(undefined as any)).toBe(false);
  });
});
