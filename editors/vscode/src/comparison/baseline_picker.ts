import * as vscode from "vscode";
import type { PinnedCommit } from "./git_source";

export interface BaselinePick extends vscode.QuickPickItem {
  mode?: "previous" | "revision" | "ref" | "expression" | "file" | "config";
  revision?: PinnedCommit;
  expression?: string;
  available?: boolean;
  action?: "more";
}

/** One native picker accepts a local ref or a listed baseline; typing never queries Git. */
export function pickBaseline(after: string, actions: BaselinePick[], load: (more: boolean, publish: (items: BaselinePick[]) => void) => Promise<BaselinePick[]>, token: vscode.CancellationToken): Promise<BaselinePick | undefined> {
  if (token.isCancellationRequested) return Promise.resolve(undefined);
  return new Promise(resolve => {
    const picker = vscode.window.createQuickPick<BaselinePick>();
    picker.title = `Show mod changes · After: ${after}`;
    picker.placeholder = "Choose Before, or type a local Git revision";
    picker.matchOnDescription = true;
    picker.matchOnDetail = true;
    let closed = false, revisions: BaselinePick[] = [{ label: "Loading local Git revisions…" }];
    let loadFailure: BaselinePick | undefined;
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
        ...(expression ? [{ label: `Use revision: ${expression}`, mode: "expression" as const, expression }] : []),
        ...revisions.filter(item => item.mode === "previous" || !item.mode && !item.action),
        ...actions, ...revisions.filter(item => item.mode !== "previous" && (item.mode || item.action)),
      ];
      if (active) {
        const retained = picker.items.find(item => item.mode === active.mode && item.action === active.action && item.label === active.label &&
          item.revision?.hash === active.revision?.hash && item.revision?.requested_ref === active.revision?.requested_ref && item.expression === active.expression);
        if (retained) picker.activeItems = [retained];
      }
    };
    const populate = async (more: boolean): Promise<void> => {
      picker.busy = true;
      if (loadFailure) { revisions = revisions.filter(item => item !== loadFailure); loadFailure = undefined; }
      try { revisions = await load(more, items => { if (!closed) { revisions = items; render(); } }); }
      catch (error) {
        if (!closed) {
          loadFailure = { label: more ? "More commits unavailable" : "Git revisions unavailable", description: error instanceof Error ? error.message : String(error) };
          revisions = more ? [...revisions, loadFailure] : [loadFailure];
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
