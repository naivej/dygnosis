/** Retain action identity and engine causes until the explicit UI boundary. */
export class ActionFailure extends Error {
  constructor(message: string, readonly code?: string | number, readonly details?: unknown) { super(message); }
}

export function fileLabel(uri: string): string {
  try { return decodeURIComponent(new URL(uri).pathname.split("/").at(-1)!) || uri; }
  catch { return uri.replaceAll("\\", "/").split("/").at(-1) || uri; }
}

export function responseFailure(value: Record<string, unknown>, action?: string, subject?: string): ActionFailure {
  const message = String(value.error ?? value.message);
  return new ActionFailure(`${action ? `${action}${subject ? ` for ${fileLabel(subject)}` : ""}: ` : ""}${message}`,
    typeof value.code === "string" || typeof value.code === "number" ? value.code : undefined, value);
}

export function failureText(error: unknown): string { return error instanceof Error ? error.message : String(error); }

export function reasonText(reason: { message: string; location?: unknown }): string {
  const site = reason.location as { uri?: unknown; range?: { start?: { line?: unknown } } } | null;
  const line = site?.range?.start?.line;
  return typeof site?.uri === "string" && typeof line === "number" && Number.isSafeInteger(line) && line >= 0
    ? `${fileLabel(site.uri)}:${line + 1} — ${reason.message}` : reason.message;
}

export function recoveryActions(error: unknown): { label: string; command: string }[] {
  if (!(error instanceof ActionFailure) || typeof error.code !== "string") return [];
  if (error.code.startsWith("ANALYSIS_")) return [{ label: "Analysis Configuration", command: "dygnosis.selectAnalysisConfiguration" }];
  if (["ENGINE_UNSUPPORTED", "ENGINE_INCOMPATIBLE", "ENGINE_PATH_UNAVAILABLE"].includes(error.code)) return [{ label: "Engine Settings", command: "dygnosis.openSettings" }];
  if (error.code === "ENGINE_UNAVAILABLE") return [{ label: "Restart server", command: "dygnosis.restartServer" }];
  return [];
}
