import * as vscode from "vscode";
import type { DygnosisClient } from "./client";
import { ActionFailure, failureText, recoveryActions } from "./failure";

export class CommandCancelled extends Error {}

export function refuse(cause: string, code?: string, details?: unknown): never { throw new ActionFailure(cause, code, details); }

/** Only an explicit command boundary presents a helper's failure. */
export async function explainFailure(service: Pick<DygnosisClient, "log">, error: unknown): Promise<void> {
  if (error instanceof CommandCancelled) return;
  service.log(String(error));
  const actions = recoveryActions(error);
  const selected = await vscode.window.showInformationMessage(failureText(error), ...actions.map(action => action.label));
  const action = actions.find(action => action.label === selected);
  if (action) await vscode.commands.executeCommand(action.command);
}

export async function runCommand<T>(service: Pick<DygnosisClient, "log">,
  action: () => T | PromiseLike<T>, active: () => boolean = () => true): Promise<T | undefined> {
  if (!active()) return undefined;
  try { return await action(); }
  catch (error) { if (active()) await explainFailure(service, error); return undefined; }
}
