/** Validate that a URL is safe to open with the system handler */
export function isAllowedUrl(url: unknown): boolean {
  if (typeof url !== "string" || url.length === 0) return false;
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

/** Validate that a filepath is safe to open with the system handler */
export function isAllowedFilepath(filepath: unknown): boolean {
  if (typeof filepath !== "string" || filepath.length === 0) return false;
  // Must be an absolute path
  return filepath.startsWith("/") || /^[A-Za-z]:\\/.test(filepath);
}
