import * as vscode from "vscode";
import type { DygnosisClient } from "./client";

export class CommandCancelled extends Error {}

export function refuse(cause: string): never { throw new Error(cause); }

/** Only an explicit command boundary presents a helper's failure. */
export async function explainFailure(service: Pick<DygnosisClient, "log">, error: unknown): Promise<void> {
  if (error instanceof CommandCancelled) return;
  service.log(String(error));
  await vscode.window.showInformationMessage(error instanceof Error ? error.message : String(error));
}

export async function runCommand<T>(service: Pick<DygnosisClient, "log">,
  action: () => T | PromiseLike<T>, active: () => boolean = () => true): Promise<T | undefined> {
  if (!active()) return undefined;
  try { return await action(); }
  catch (error) { if (active()) await explainFailure(service, error); return undefined; }
}
