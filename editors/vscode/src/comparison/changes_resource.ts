import { analysisConfiguration, record } from "../client/protocol";
import type { AnalysisConfiguration } from "../client/protocol";
import { configurationSummary } from "../client/analysis_presentation";

interface AnalysisSelector { analysis?: AnalysisConfiguration; search_paths?: string[] }
export interface WorkingSelector extends AnalysisSelector { kind: "working"; root_uri: string }
export interface GitSelector extends AnalysisSelector {
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
  if (value.analysis !== undefined && !analysisConfiguration(value.analysis) || value.search_paths !== undefined &&
      (!Array.isArray(value.search_paths) || value.search_paths.some(path => typeof path !== "string" || path.includes("\0")))) return false;
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
  const segments = (input: ModelSelector): string[] => {
    const uri = new URL(input.kind === "git" ? input.repository_uri : input.root_uri);
    return [uri.protocol === "untitled:" ? "Untitled" : uri.host, ...decodeURIComponent(uri.pathname).split("/"),
      ...(input.kind === "git" ? input.root_file.split("/") : [])].filter(Boolean);
  };
  const beforePath = segments(resource.before), afterPath = segments(resource.after);
  const before = beforePath.at(-1)!, after = afterPath.at(-1)!;
  const version = (input: ModelSelector): string => {
    // A tab caption is one URI path segment, even when a configuration name contains a slash.
    const configuration = input.analysis ? configurationLabel(input.analysis).replaceAll("/", "∕").replaceAll("\\", "∖") : undefined;
    if (input.kind === "working") return configuration ?? "Working";
    return [input.commit.slice(0, 7), configuration].filter(Boolean).join(" · ");
  };
  if (beforePath.join("/") === afterPath.join("/")) {
    return `${after} · ${version(resource.before)} → ${version(resource.after)}.dygnosis-changes`;
  }
  let beforeLabel = before, afterLabel = after;
  if (before === after) {
    let depth = 2;
    while (beforePath.at(-depth) === afterPath.at(-depth)) depth++;
    beforeLabel = `${before} (${beforePath.at(-depth) ?? "Root"})`;
    afterLabel = `${after} (${afterPath.at(-depth) ?? "Root"})`;
  }
  const side = (input: ModelSelector, label: string): string => {
    const detail = version(input);
    return input.kind === "git" || detail !== "Source" && detail !== "Working" ? `${label} · ${detail}` : label;
  };
  return `${side(resource.before, beforeLabel)} → ${side(resource.after, afterLabel)}.dygnosis-changes`;
}
export function configurationLabel(analysis: AnalysisConfiguration): string {
  return configurationSummary(analysis);
}
