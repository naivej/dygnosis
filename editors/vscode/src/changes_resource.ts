import { createHash } from "node:crypto";
import { record } from "./protocol";

export interface WorkingSelector { kind: "working"; root_uri: string }
export interface GitSelector {
  kind: "git"; repository_uri: string; commit: string; root_file: string; requested_ref: string;
}
export type ModelSelector = WorkingSelector | GitSelector;
export interface ComparisonResource {
  schema_version: 1; before: ModelSelector; after: ModelSelector; anchor: ModelSelector; context_uri: string;
}
export const changesScheme = "dygnosis-changes", changesViewType = "dygnosis.changes";
export const historyScheme = "dygnosis-history";

export function treeKey(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 32768 &&
    !value.includes("\0") && !value.includes("\\") && !value.startsWith("/") &&
    !/^[A-Za-z]:/.test(value) && value.split("/").every(part => part !== "" && part !== "." && part !== "..");
}
function sourceUri(value: unknown, untitled = false): value is string {
  if (typeof value !== "string" || value.length > 65536) return false;
  try { const url = new URL(value); return url.protocol === "file:" || untitled && url.protocol === "untitled:"; } catch { return false; }
}
export function selector(value: unknown): value is ModelSelector {
  if (!record(value)) return false;
  if (value.kind === "working") return sourceUri(value.root_uri, true) && /\.(mod|dyn)$/i.test(new URL(value.root_uri).pathname);
  return value.kind === "git" && sourceUri(value.repository_uri) && typeof value.commit === "string" &&
    /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(value.commit) && treeKey(value.root_file) && /\.(mod|dyn)$/i.test(value.root_file) &&
    typeof value.requested_ref === "string" && value.requested_ref.length <= 4096 && !value.requested_ref.includes("\0");
}
/** Selectors survive reload; source bytes and capture generations never enter the URI. */
export function resourceData(value: unknown): ComparisonResource {
  if (!record(value) || value.schema_version !== 1 || !selector(value.before) || !selector(value.after) ||
      !selector(value.anchor) || !sourceUri(value.context_uri, true)) throw new Error("This saved comparison has unsupported inputs. Open changes from a model again.");
  return { schema_version: 1, before: value.before, after: value.after, anchor: value.anchor, context_uri: value.context_uri };
}
export function resourceQuery(resource: ComparisonResource): string { return JSON.stringify(resourceData(resource)); }
export function resourceName(resource: ComparisonResource): string {
  const base = resource.after.kind === "git" ? resource.after.root_file.split("/").at(-1)! : decodeURIComponent(new URL(resource.after.root_uri).pathname.split("/").at(-1)!);
  const version = (input: ModelSelector): string => input.kind === "git" ? input.commit.slice(0, 7) : "Working";
  return `${base} · ${version(resource.before)} → ${version(resource.after)}.dygnosis-changes`;
}
export function selectorId(input: ModelSelector): string { return createHash("sha256").update(JSON.stringify(input)).digest("hex"); }
export function inputLabel(input: ModelSelector): string {
  if (input.kind === "working") return `${decodeURIComponent(new URL(input.root_uri).pathname)} · Working`;
  const commit = input.commit.slice(0, 7), ref = input.requested_ref.replace(/^[0-9a-f]{40,64}(?=$|[~^])/, value => value.slice(0, 7));
  return `${input.root_file}${ref && ref !== commit ? ` @ ${ref}` : ""} · ${commit}`;
}
