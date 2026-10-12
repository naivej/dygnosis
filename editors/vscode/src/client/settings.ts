import * as vscode from "vscode";

export const outlineSections = ["declarations", "blocks", "commands", "dimensions", "equations"];
export type Log = (message: string) => void;
/** Macro tint changes no server input; simultaneous engine-setting changes still do. */
export function macroTintOnlyChange(event: vscode.ConfigurationChangeEvent, resource?: vscode.Uri): boolean {
  return event.affectsConfiguration("dynare.effectiveModel.macroTint", resource) &&
    !["serverPath", "searchPaths", "formatIndent", "projectDiagnostics", "projectExcludePaths", "nameDetails", "outline", "parameterValueHints"]
      .some(key => event.affectsConfiguration(`dynare.${key}`, resource));
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
  log(`Invalid dynare.${key}; using ${String(fallback)}.`);
  return fallback;
}
export function listSetting(key: string, resource: vscode.Uri | undefined, allowed: string[], fallback: string[], log: Log): string[] {
  const value = valueSetting(key, resource, fallback);
  if (!Array.isArray(value)) { log(`Invalid dynare.${key}; using the default.`); return [...fallback]; }
  const result = [...new Set(value.filter((item): item is string => typeof item === "string" && allowed.includes(item)))];
  if (result.length !== value.length) log(`Invalid or repeated entries in dynare.${key} were ignored.`);
  return result;
}
export function projectDiagnosticsSetting(log: Log): boolean {
  const value: unknown = vscode.workspace.getConfiguration("dynare").get("projectDiagnostics", true);
  if (typeof value === "boolean") return value;
  log("Invalid dynare.projectDiagnostics; using true.");
  return true;
}
function stringListSetting(key: string, resource: vscode.Uri | undefined, log: Log): string[] {
  const raw = valueSetting(key, resource, []);
  const values = Array.isArray(raw) ? raw.filter((item): item is string => typeof item === "string") : [];
  if (!Array.isArray(raw) || values.length !== raw.length) log(`Invalid dynare.${key} entries were ignored.`);
  return values;
}
export function engineSettings(resource: vscode.Uri | undefined, log: Log) {
  const searchPaths = stringListSetting("searchPaths", resource, log);
  const rawIndent = valueSetting("formatIndent", resource, "tab");
  const formatIndent = rawIndent === "tab" || (typeof rawIndent === "number" && Number.isInteger(rawIndent) && rawIndent >= 1 && rawIndent <= 8) ? rawIndent : "tab";
  if (formatIndent !== rawIndent) log("Invalid dynare.formatIndent; using a tab.");
  return {
    searchPaths, formatIndent, projectExcludePaths: stringListSetting("projectExcludePaths", resource, log),
    nameDetails: {
      longName: booleanSetting("nameDetails.longName", resource, true, log),
      tex: booleanSetting("nameDetails.tex", resource, true, log),
    },
    outline: {
      sections: listSetting("outline.sections", resource, outlineSections, outlineSections, log),
      equationNumbers: booleanSetting("outline.equationNumbers", resource, true, log),
    },
    parameterValueHints: booleanSetting("parameterValueHints", resource, true, log),
  };
}
export function configurationSnapshot(log: Log) {
  return { dynare: { configuration: {
    schemaVersion: 1,
    loose: { ...engineSettings(undefined, log), projectDiagnostics: projectDiagnosticsSetting(log) },
    folders: (vscode.workspace.workspaceFolders ?? []).filter(folder => folder.uri.scheme === "file").map(folder => ({
      uri: folder.uri.toString(), settings: engineSettings(folder.uri, log),
    })),
  } } };
}
