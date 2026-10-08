import * as vscode from "vscode";
import { GitSelector, historyScheme, treeKey } from "./changes_resource";
import { record } from "./protocol";

export interface HistoricalSource { repository_uri: string; commit: string; file_key: string }
interface RetainedSource { text: string; holders: Set<object>; owners: Map<string, GitSelector> }
export function historicalSource(uri: vscode.Uri): HistoricalSource | undefined {
  if (uri.scheme !== historyScheme || uri.query.length > 131072) return undefined;
  try {
    const value: unknown = JSON.parse(uri.query);
    if (!record(value) || value.schema_version !== 1 || typeof value.repository_uri !== "string" ||
        vscode.Uri.parse(value.repository_uri).scheme !== "file" || typeof value.commit !== "string" ||
        !/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(value.commit) || !treeKey(value.file_key)) return undefined;
    return { repository_uri: value.repository_uri, commit: value.commit, file_key: value.file_key };
  } catch { return undefined; }
}
export function historicalUri(input: HistoricalSource): vscode.Uri {
  const basename = input.file_key.split("/").at(-1)!;
  return vscode.Uri.from({ scheme: historyScheme, path: `/${basename} · ${input.commit.slice(0, 7)}`,
    query: JSON.stringify({ schema_version: 1, ...input }) });
}

/** Open source tabs keep their captured bytes after the comparison document closes. */
export class HistoricalSources implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly entries = new Map<string, RetainedSource>();
  private readonly registration: vscode.Disposable;
  private readonly closing: vscode.Disposable;
  constructor(private readonly load: (input: HistoricalSource, token: vscode.CancellationToken) => Promise<string>) {
    this.registration = vscode.workspace.registerTextDocumentContentProvider(historyScheme, this);
    this.closing = vscode.workspace.onDidCloseTextDocument(document => this.prune(document.uri.toString()));
  }
  retain(owner: object, input: GitSelector, sources: Record<string, string>): Map<string, vscode.Uri> {
    const uris = new Map<string, vscode.Uri>();
    for (const [file_key, text] of Object.entries(sources)) {
      if (!treeKey(file_key)) throw new Error("The engine returned an invalid historical source key.");
      const uri = historicalUri({ repository_uri: input.repository_uri, commit: input.commit, file_key });
      const key = uri.toString(), existing = this.entries.get(key);
      if (existing && existing.text !== text) throw new Error("Historical source bytes differ for the same commit and path.");
      const entry = existing ?? { text, holders: new Set<object>(), owners: new Map<string, GitSelector>() };
      entry.holders.add(owner); entry.owners.set(input.root_file, input); this.entries.set(key, entry); uris.set(file_key, uri);
    }
    return uris;
  }
  owners(uri: vscode.Uri): GitSelector[] { return [...(this.entries.get(uri.toString())?.owners.values() ?? [])]; }
  release(owner: object): void {
    for (const [key, entry] of this.entries) { entry.holders.delete(owner); this.prune(key); }
  }
  private prune(key: string): void {
    const entry = this.entries.get(key);
    if (entry && !entry.holders.size && !vscode.workspace.textDocuments.some(document => !document.isClosed && document.uri.toString() === key)) this.entries.delete(key);
  }
  async provideTextDocumentContent(uri: vscode.Uri, token: vscode.CancellationToken): Promise<string> {
    const entry = this.entries.get(uri.toString());
    if (entry) return entry.text;
    const input = historicalSource(uri);
    if (!input) throw new Error("This historical source has invalid provenance. Choose its source revision again.");
    const text = await this.load(input, token);
    if (token.isCancellationRequested) throw new Error("Historical source loading was cancelled.");
    this.entries.set(uri.toString(), { text, holders: new Set(), owners: new Map() });
    return text;
  }
  dispose(): void { this.closing.dispose(); this.registration.dispose(); this.entries.clear(); }
}
