import * as vscode from "vscode";

export const outlineSections = ["declarations", "blocks", "commands", "dimensions", "equations"];
export const outlineActivities = ["both", "active", "inactive"] as const;
export type OutlineActivity = typeof outlineActivities[number];
export type Log = (message: string) => void;
const reported = new WeakMap<Log, Set<string>>();
function invalidSetting(key: string, resource: vscode.Uri | undefined, expected: string, outcome: string, log: Log): void {
  const message = `Invalid dynare.${key} (${resource?.toString().slice(0, 512) ?? "user settings"}): expected ${expected}; ${outcome}.`;
  let seen = reported.get(log); if (!seen) reported.set(log, seen = new Set());
  if (seen.has(message)) return;
  if (seen.size >= 128) seen.delete(seen.values().next().value!);
  seen.add(message); log(message);
}
/** Settings that can change model inputs or the set of workspace owners. */
export function analysisSettingsChanged(event: vscode.ConfigurationChangeEvent, resource?: vscode.Uri): boolean {
  return ["searchPaths", "macroWorkLimit", "workspaceDiagnostics", "workspaceExcludePaths"]
    .some(key => event.affectsConfiguration(`dynare.${key}`, resource));
}
export function serverSettingsChanged(event: vscode.ConfigurationChangeEvent): boolean {
  return analysisSettingsChanged(event) || ["formatIndent", "nameDetails", "outline", "parameterValueHints"]
    .some(key => event.affectsConfiguration(`dynare.${key}`));
}
function valueSetting(key: string, resource: vscode.Uri | undefined, fallback: unknown): unknown {
  const config = vscode.workspace.getConfiguration("dynare", { uri: resource, languageId: "dynare" });
  if (resource) return config.get<unknown>(key, fallback);
  const inspected = config.inspect<unknown>(key);
  return inspected?.globalLanguageValue ?? inspected?.globalValue ?? inspected?.defaultLanguageValue ?? inspected?.defaultValue ?? fallback;
}
export function booleanSetting(key: string, resource: vscode.Uri | undefined, fallback: boolean, log: Log): boolean {
  const value = valueSetting(key, resource, fallback);
  if (typeof value === "boolean") return value;
  invalidSetting(key, resource, "true or false", `using ${fallback}`, log);
  return fallback;
}
export function outlineActivitySetting(resource: vscode.Uri | undefined, log: Log): OutlineActivity {
  const value = valueSetting("outline.activity", resource, "both");
  if (typeof value === "string" && outlineActivities.some(option => option === value)) return value as OutlineActivity;
  invalidSetting("outline.activity", resource, "both, active or inactive", "using both", log);
  return "both";
}
export function listSetting(key: string, resource: vscode.Uri | undefined, allowed: string[], fallback: string[], log: Log): string[] {
  const value = valueSetting(key, resource, fallback);
  if (!Array.isArray(value)) { invalidSetting(key, resource, `a list of ${allowed.join(", ")}`, `using ${fallback.join(", ")}`, log); return [...fallback]; }
  const result = [...new Set(value.filter((item): item is string => typeof item === "string" && allowed.includes(item)))];
  if (result.length !== value.length) invalidSetting(key, resource, `unique entries from ${allowed.join(", ")}`, "invalid or repeated entries ignored", log);
  return result;
}
export function workspaceDiagnosticsSetting(log: Log): boolean {
  const value: unknown = vscode.workspace.getConfiguration("dynare").get("workspaceDiagnostics", true);
  if (typeof value === "boolean") return value;
  invalidSetting("workspaceDiagnostics", undefined, "true or false", "using true", log);
  return true;
}
function stringListSetting(key: string, resource: vscode.Uri | undefined, log: Log): string[] {
  const raw = valueSetting(key, resource, []);
  const values = Array.isArray(raw) ? raw.filter((item): item is string => typeof item === "string") : [];
  if (!Array.isArray(raw) || values.length !== raw.length) invalidSetting(key, resource, 'a list of strings, for example ["models"]', "non-string entries ignored", log);
  return values;
}
export function engineSettings(resource: vscode.Uri | undefined, log: Log) {
  const searchPaths = stringListSetting("searchPaths", resource, log);
  const rawIndent = valueSetting("formatIndent", resource, "tab");
  const formatIndent = rawIndent === "tab" || (typeof rawIndent === "number" && Number.isInteger(rawIndent) && rawIndent >= 1 && rawIndent <= 8) ? rawIndent : "tab";
  if (formatIndent !== rawIndent) invalidSetting("formatIndent", resource, "tab or an integer from 1 to 8", "using tab", log);
  return {
    searchPaths, macroWorkLimit: valueSetting("macroWorkLimit", resource, 2_000_000), formatIndent, workspaceExcludePaths: stringListSetting("workspaceExcludePaths", resource, log),
    nameDetails: {
      longName: booleanSetting("nameDetails.longName", resource, true, log),
      tex: booleanSetting("nameDetails.tex", resource, true, log),
    },
    outline: {
      sections: listSetting("outline.sections", resource, outlineSections, outlineSections, log),
      equationNumbers: booleanSetting("outline.equationNumbers", resource, true, log),
      activity: outlineActivitySetting(resource, log),
    },
    parameterValueHints: booleanSetting("parameterValueHints", resource, true, log),
  };
}
export function configurationSnapshot(log: Log) {
  return { dynare: { configuration: {
    schemaVersion: 1,
    loose: { ...engineSettings(undefined, log), workspaceDiagnostics: workspaceDiagnosticsSetting(log) },
    folders: (vscode.workspace.workspaceFolders ?? []).filter(folder => folder.uri.scheme === "file").map(folder => ({
      uri: folder.uri.toString(), settings: engineSettings(folder.uri, log),
    })),
  } } };
}
