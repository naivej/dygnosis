import * as vscode from "vscode";
import type { MessageSignature, Middleware } from "vscode-languageclient/node";
import { sourceUriKey } from "./source_uri";

interface Activity { label: string; priority: number }
interface ModelTarget { root?: string; document?: string }
interface PendingActivity extends Activity, ModelTarget {
  ready: boolean; timer?: ReturnType<typeof setTimeout>; cancellation?: vscode.Disposable;
}
export interface ModelActivity { label: string; pending: number; detail: string }

const requests: Record<string, string> = {
  "textDocument/hover": "Loading hover",
  "textDocument/completion": "Loading completions",
  "textDocument/signatureHelp": "Loading signature help",
  "textDocument/definition": "Finding definition",
  "textDocument/declaration": "Finding declaration",
  "textDocument/typeDefinition": "Finding type definition",
  "textDocument/references": "Finding references",
  "textDocument/documentHighlight": "Finding uses",
  "textDocument/linkedEditingRange": "Finding linked names",
  "textDocument/selectionRange": "Loading selections",
  "textDocument/prepareCallHierarchy": "Finding calls",
  "callHierarchy/incomingCalls": "Finding callers",
  "callHierarchy/outgoingCalls": "Finding calls",
  "textDocument/documentSymbol": "Updating Outline",
  "textDocument/foldingRange": "Updating folds",
  "textDocument/documentLink": "Loading source links",
  "textDocument/codeAction": "Loading code actions",
  "textDocument/prepareRename": "Checking Rename",
  "textDocument/rename": "Preparing Rename",
  "textDocument/formatting": "Formatting document",
  "textDocument/rangeFormatting": "Formatting selection",
  "textDocument/diagnostic": "Checking model",
  "textDocument/semanticTokens/full": "Updating syntax colors",
  "textDocument/semanticTokens/full/delta": "Updating syntax colors",
  "textDocument/semanticTokens/range": "Updating syntax colors",
  "textDocument/inlayHint": "Updating parameter hints",
};
const commands: Record<string, string> = {
  "dynare/showEffectiveModel": "Expanding macros",
  "dynare/modelInfo": "Updating model information",
  "dynare/analysisContext": "Loading configuration",
  "dynare/setAnalysisConfiguration": "Changing configuration",
  "dynare/setDocumentContext": "Selecting model",
  "dynare/captureRename": "Preparing Rename",
  "dynare/sourceCandidates": "Finding source candidates",
};
function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object"; }
function key(value: string): string | undefined {
  return /^(?:file|untitled):/.test(value) ? sourceUriKey(vscode.Uri.parse(value)) : undefined;
}
function target(value: unknown, rootForDocument: (document: string) => string | undefined): ModelTarget | undefined {
  if (typeof value === "string") { const root = key(value); return root ? { root } : undefined; }
  if (!record(value)) return undefined;
  const document = typeof value.document_uri === "string" ? value.document_uri
    : record(value.textDocument) && typeof value.textDocument.uri === "string" ? value.textDocument.uri
      : record(value.item) && typeof value.item.uri === "string" ? value.item.uri : undefined;
  const root = typeof value.root_uri === "string" ? value.root_uri : document && rootForDocument(document);
  const rootKey = root && key(root), documentKey = document && key(document);
  return rootKey || documentKey ? { root: rootKey, document: documentKey } : undefined;
}
function activity(type: string | MessageSignature, param: unknown, rootForDocument: (document: string) => string | undefined): Activity & ModelTarget | undefined {
  const method = typeof type === "string" ? type : type.method;
  if (method.startsWith("workspace/") && method !== "workspace/executeCommand") return undefined;
  if (method === "workspace/executeCommand" && record(param) && typeof param.command === "string") {
    if (/^dynare\/(?:workspace|recheckWorkspace|cancelWorkspace|compare|validateComparison)/.test(param.command)) return undefined;
    const selected = Array.isArray(param.arguments) && param.arguments.length === 1 ? target(param.arguments[0], rootForDocument) : undefined;
    if (!selected) return undefined;
    const label = commands[param.command];
    return { ...selected, label: label ?? "Working", priority: label && param.command !== "dynare/modelInfo" ? 1 : 0 };
  }
  const selected = target(param, rootForDocument);
  return selected ? { ...selected, label: requests[method] ?? "Working", priority: 0 } : undefined;
}

/** Delayed model request state; the existing model status item owns its display. */
export class RequestActivity implements vscode.Disposable {
  private readonly pending = new Map<object, PendingActivity>();
  private readonly changed = new vscode.EventEmitter<void>();
  readonly onDidChange = this.changed.event;
  private generation = 0;
  private disposed = false;

  middleware(base: Middleware, rootForDocument: (document: string) => string | undefined = () => undefined): Middleware {
    this.reset();
    const generation = this.generation;
    // Feature middleware remains live when another editor feature installs it.
    const middleware = Object.create(base) as Middleware;
    middleware.sendRequest = async (type, param, token, next) => {
      const finish = this.begin(activity(type, param, rootForDocument), generation, token);
      try { return await (base.sendRequest ? base.sendRequest(type, param, token, next) : next(type, param, token)); }
      finally { finish(); }
    };
    return middleware;
  }

  private begin(value: Activity & ModelTarget | undefined, generation: number, token?: vscode.CancellationToken): () => void {
    if (!value || this.disposed || generation !== this.generation || token?.isCancellationRequested) return () => {};
    const model = value.root ?? value.document;
    const ready = [...this.pending.values()].some(entry => entry.ready && (entry.root ?? entry.document) === model);
    const request = {}, entry: PendingActivity = { ...value, ready };
    const finish = (): void => {
      if (!this.pending.delete(request)) return;
      if (entry.timer) clearTimeout(entry.timer);
      entry.cancellation?.dispose();
      if (entry.ready) this.changed.fire();
    };
    this.pending.set(request, entry);
    entry.cancellation = token?.onCancellationRequested(finish);
    if (ready) this.changed.fire();
    else entry.timer = setTimeout(() => {
      if (!this.pending.has(request) || entry.ready) return;
      // Once this model is visibly busy, overlapping work keeps it busy.
      // Each unrelated model must still pass its own delay.
      for (const pending of this.pending.values()) if ((pending.root ?? pending.document) === model) {
        if (pending.timer) clearTimeout(pending.timer);
        pending.timer = undefined;
        pending.ready = true;
      }
      this.changed.fire();
    }, 250);
    return finish;
  }

  forDocument(document: vscode.Uri, root?: vscode.Uri): ModelActivity | undefined {
    const documentKey = sourceUriKey(document), rootKey = root && sourceUriKey(root);
    let current: Activity | undefined;
    let pending = 0;
    const labels = new Map<string, number>();
    for (const entry of this.pending.values()) {
      const matches = entry.root ? entry.root === (rootKey ?? documentKey)
        : entry.document === documentKey || !!rootKey && entry.document === rootKey;
      if (!entry.ready || !matches) continue;
      if (!current || entry.priority > current.priority) current = entry;
      ++pending;
      labels.set(entry.label, (labels.get(entry.label) ?? 0) + 1);
    }
    return current && { label: current.label, pending,
      detail: `Waiting for the language server:\n${[...labels].map(([label, count]) => `• ${label}${count > 1 ? ` (${count})` : ""}`).join("\n")}` };
  }

  reset(): void {
    ++this.generation;
    const changed = [...this.pending.values()].some(entry => entry.ready);
    for (const entry of this.pending.values()) {
      if (entry.timer) clearTimeout(entry.timer);
      entry.cancellation?.dispose();
    }
    this.pending.clear();
    if (changed) this.changed.fire();
  }

  dispose(): void {
    this.disposed = true;
    this.reset();
    this.changed.dispose();
  }
}
