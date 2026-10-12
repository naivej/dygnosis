import * as vscode from "vscode";
import { randomBytes } from "node:crypto";

const scheme = "dygnosis-captured";
/** Read-only exact text actions use a capture registry, never a webview path. */
export class CapturedTextSources implements vscode.Disposable {
  private readonly entries = new Map<string, { text: string; holders: Set<object> }>();
  private readonly held = new Map<object, Map<string, vscode.Uri>>();
  private readonly subscriptions: vscode.Disposable[];
  constructor() {
    this.subscriptions = [vscode.workspace.registerTextDocumentContentProvider(scheme, {
      provideTextDocumentContent: uri => { const entry = this.entries.get(uri.toString()); if (!entry) throw new Error("This captured source is unavailable. Refresh the comparison."); return entry.text; },
    }), vscode.workspace.onDidCloseTextDocument(document => { const entry = this.entries.get(document.uri.toString()); if (entry && !entry.holders.size) this.entries.delete(document.uri.toString()); })];
  }
  retain(holder: object, identity: string, label: string, text: string): vscode.Uri {
    let map = this.held.get(holder); if (!map) { map = new Map(); this.held.set(holder, map); }
    const existing = map.get(identity); if (existing) return existing;
    const uri = vscode.Uri.from({ scheme, path: `/${randomBytes(16).toString("hex")}/${label.replace(/[\\/\0\r\n]/g, "_")}` });
    this.entries.set(uri.toString(), { text, holders: new Set([holder]) }); map.set(identity, uri); return uri;
  }
  release(holder: object): void {
    const map = this.held.get(holder); if (!map) return; this.held.delete(holder);
    for (const uri of map.values()) { const entry = this.entries.get(uri.toString()); if (!entry) continue; entry.holders.delete(holder); if (!vscode.workspace.textDocuments.some(document => !document.isClosed && document.uri.toString() === uri.toString())) this.entries.delete(uri.toString()); }
  }
  dispose(): void { for (const subscription of this.subscriptions) subscription.dispose(); this.entries.clear(); this.held.clear(); }
}
