import * as path from "node:path";

/** Compatibility boundary for the built-in Git extension's URI query, not a native selector. */
export interface GitDocumentUri { scheme: string; query: string; authority?: string }
export type GitDocumentIdentity =
  | { kind: "commit"; path: string; ref: string }
  | { kind: "unavailable"; code: string; message: string };

export function decodeGitDocument(uri: GitDocumentUri): GitDocumentIdentity {
  const unavailable = (message: string, code = "unknown_provenance"): GitDocumentIdentity => ({ kind: "unavailable", code, message });
  if (uri.scheme !== "git" || uri.authority || uri.query.length > 32 * 1024) return unavailable("Select a source revision for this document.");
  let value: unknown;
  try { value = JSON.parse(uri.query); } catch { return unavailable("The Git document has an invalid source identity. Select a source revision."); }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return unavailable("The Git document has an invalid source identity. Select a source revision.");
  const record = value as Record<string, unknown>;
  if (typeof record.path !== "string" || !path.isAbsolute(record.path) || /[\0\r\n]/.test(record.path) || typeof record.ref !== "string") return unavailable("The Git document has an invalid source identity. Select a source revision.");
  if (record.submoduleOf !== undefined) return unavailable("A submodule change document is not a fixed model source. Open a file in the submodule repository.", "unsupported_source");
  if (record.ref === "" || /^:[0-3]$/.test(record.ref) || record.ref === "~") return unavailable("Index and merge-stage documents cannot supply a model revision.", "unsupported_source");
  // A branch name does not prove which commit supplied a document that is already open.
  if (!/^(?:[a-fA-F0-9]{7,64})(?:[~^]\d*)*$/.test(record.ref)) return unavailable("The Git document does not identify a fixed commit. Select its source revision.");
  return { kind: "commit", path: record.path, ref: record.ref };
}

/** One source form is used by comparison, read-only source tabs, and provenance checks. */
export function normalizeGitText(text: string): string { return text.replace(/\r(?!\n)/g, "\n"); }

export function decodeGitText(bytes: Buffer): string {
  let text: string;
  try { text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes); }
  catch { text = bytes.toString("latin1"); }
  return normalizeGitText(text);
}
