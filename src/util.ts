// Small shared helpers.

/** Return the last `maxBytes` bytes of `s` (UTF-8), as a string. */
export function tail(s: string, maxBytes: number): string {
  const buf = Buffer.from(s, "utf8");
  if (buf.length <= maxBytes) return s;
  return buf.subarray(buf.length - maxBytes).toString("utf8");
}

/** Clamp a wait/timeout value to the documented [1000, 600000] ms window. */
export function clampWait(ms: number): number {
  return Math.min(600000, Math.max(1000, ms));
}
