import * as vscode from "vscode";
import { GitSourceError, type PinnedCommit } from "./git_source";
import { failureText } from "../client/failure";

export interface BaselinePick extends vscode.QuickPickItem {
  mode?: "revision" | "ref" | "expression" | "file" | "config";
  revision?: PinnedCommit;
  expression?: string;
  action?: "more";
}

/** One native picker accepts a local ref or a listed baseline; typing never queries Git. */
export function pickBaseline(title: string, actions: BaselinePick[], load: (more: boolean, publish: (items: BaselinePick[], gitAvailable?: boolean) => void) => Promise<BaselinePick[]>, token: vscode.CancellationToken): Promise<BaselinePick | undefined> {
  if (token.isCancellationRequested) return Promise.resolve(undefined);
  return new Promise(resolve => {
    const picker = vscode.window.createQuickPick<BaselinePick>();
    picker.title = title;
    picker.placeholder = "Choose Before";
    picker.matchOnDescription = true;
    picker.matchOnDetail = true;
    let closed = false, revisions: BaselinePick[] = [], gitAvailable = false;
    const subscriptions: vscode.Disposable[] = [];
    const finish = (choice?: BaselinePick): void => {
      if (closed) return;
      closed = true;
      for (const subscription of subscriptions) subscription.dispose();
      picker.dispose();
      resolve(token.isCancellationRequested ? undefined : choice);
    };
    const render = (preserveActive = true): void => {
      if (closed) return;
      const active = preserveActive ? picker.activeItems[0] : undefined;
      const expression = picker.value.trim();
      picker.items = [
        ...(expression && gitAvailable ? [{ label: `Use revision: ${expression}`, mode: "expression" as const, expression }] : []),
        ...actions, ...revisions.filter(item => item.mode || item.action === "more" && !picker.busy),
      ];
      const retained = active && picker.items.find(item => item.mode === active.mode && item.action === active.action && item.label === active.label &&
        item.revision?.hash === active.revision?.hash && item.revision?.requested_ref === active.revision?.requested_ref && item.expression === active.expression);
      const next = retained ?? picker.items[0];
      if (next) picker.activeItems = [next];
    };
    const populate = async (more: boolean): Promise<void> => {
      picker.busy = true;
      render();
      try { revisions = await load(more, (items, available) => { if (!closed) { revisions = items; gitAvailable ||= available === true; render(); } }); }
      catch (error) {
        const unavailable = error instanceof GitSourceError && ["git_unavailable", "repository_unavailable"].includes(error.code);
        if (!closed && (more || !unavailable)) {
          const action = more ? "load more Git commits" : "open the Git repository";
          void vscode.window.showInformationMessage(`Could not ${action}. ${failureText(error)}\nYou can still use the other comparison choices.`);
        }
      }
      if (closed) return;
      picker.busy = false;
      render();
    };
    subscriptions.push(picker.onDidChangeValue(() => render(false)), picker.onDidHide(() => finish()), token.onCancellationRequested(() => finish()),
      picker.onDidAccept(() => {
        const selected = picker.selectedItems[0] ?? picker.activeItems[0];
        if (!selected) return;
        if (selected.action === "more") { if (!picker.busy) void populate(true); return; }
        if (selected.mode) finish(selected);
      }));
    render();
    picker.show();
    void populate(false);
  });
}
