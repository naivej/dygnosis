import type { AnalysisConfiguration } from "./protocol";

export function automaticConfigurationName(name: string | null | undefined): boolean {
  return !name || /^(?:Analysis preview|Macro inputs)(?: \(modified\))*$/.test(name);
}
const short = (text: string): string => text.length > 70 ? text.slice(0, 67) + "…" : text;

/** Summarize accepted choices only; never resolve targets or evaluate expressions. */
export function configurationSummary(analysis: AnalysisConfiguration, paths?: string[] | null): string {
  if (!automaticConfigurationName(analysis.name)) return short(analysis.name!);
  const defines = analysis.defines ?? [];
  const parts = defines.slice(0, 2).map(input => short(`${input.name}=${input.expression}`));
  if (defines.length > 2) parts.push(`+${defines.length - 2} macro inputs`);
  const counts = { branch: 0, definition: 0, skip_definition: 0 };
  for (const operation of analysis.overrides ?? []) ++counts[operation.kind];
  for (const [kind, label] of [["branch", "branch preview"], ["definition", "definition override"], ["skip_definition", "skipped definition"]] as const) {
    const count = counts[kind];
    if (count) parts.push(`${count} ${label}${count === 1 ? "" : "s"}`);
  }
  const selection = analysis.equation_selection;
  if (selection) parts.push(short(`${selection.mode === "include" ? "Include" : "Exclude"} ${selection.tag ?? "name"}=${selection.values.slice(0, 2).join(",")}${selection.values.length > 2 ? ` (+${selection.values.length - 2})` : ""}`));
  if (paths !== undefined && paths !== null) parts.push(`${paths.length} include path${paths.length === 1 ? "" : "s"}`);
  return parts.length ? parts.slice(0, 2).join(" · ") + (parts.length > 2 ? ` · +${parts.length - 2} choices` : "") : "Source";
}
