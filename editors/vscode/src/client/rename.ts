import { refuse, explainFailure } from "./refusal";
import { Buffer } from "node:buffer";
import * as vscode from "vscode";
import { PrepareRenameRequest, TextDocumentEdit, WorkspaceEdit as ProtocolEdit } from "vscode-languageclient/node";
import type { DygnosisClient } from "./client";
import { record } from "./protocol";
import { sourceUriKey } from "./source_uri";

interface Source { uri: string; text: string; version: number | null }
interface Capture { edit: ProtocolEdit; sources: Source[]; owners: string[] }
const sourceLimit = 16 * 1024 * 1024;
const identity = (uri: string): string => sourceUriKey(vscode.Uri.parse(uri));

function capture(value: unknown): Capture | undefined {
  if (record(value) && typeof value.error === "string") throw new Error(value.error);
  if (!record(value) || value.schema_version !== 1 || !record(value.edit) || !Array.isArray(value.edit.documentChanges)
    || !Array.isArray(value.sources) || !Array.isArray(value.owners)) return undefined;
  const documents = value.edit.documentChanges;
  if (!documents.length || !documents.every(TextDocumentEdit.is)) return undefined;
  const documentKeys = documents.map(document => identity(document.textDocument.uri));
  if (new Set(documentKeys).size !== documents.length) return undefined;
  let bytes = 0;
  const sources: Source[] = [];
  for (const source of value.sources) {
    if (!record(source) || typeof source.uri !== "string" || typeof source.text !== "string"
      || source.version !== null && !Number.isInteger(source.version)) return undefined;
    const key = identity(source.uri);
    const document = documents.find(row => identity(row.textDocument.uri) === key);
    if (document && document.textDocument.version !== source.version || sources.some(row => identity(row.uri) === key)) return undefined;
    bytes += Buffer.byteLength(source.text);
    if (bytes > sourceLimit || !["file", "untitled"].includes(vscode.Uri.parse(source.uri).scheme)) return undefined;
    sources.push(source as unknown as Source);
  }
  if (documents.some(document => !sources.some(source => identity(source.uri) === identity(document.textDocument.uri)))) return undefined;
  const owners: string[] = [];
  for (const owner of value.owners) {
    if (!record(owner) || typeof owner.root_uri !== "string" || typeof owner.revision !== "string" || owners.includes(identity(owner.root_uri))) return undefined;
    owners.push(identity(owner.root_uri));
  }
  return owners.length ? { edit: value.edit as unknown as ProtocolEdit, sources, owners: owners.sort() } : undefined;
}

/** Keep guards alive through the public applyEdit serialization/application. */
export async function withCheckedRenameEdit<T>(service: DygnosisClient, document: vscode.TextDocument, position: vscode.Position,
  newName: string, token: vscode.CancellationToken, consume: (edit: vscode.WorkspaceEdit) => T | PromiseLike<T>): Promise<T | undefined> {
    if (token.isCancellationRequested) return undefined;
    const client = service.client;
    if (!client?.initializeResult?.capabilities.executeCommandProvider?.commands.includes("dynare/captureRename")) refuse("This engine does not support safe Rename. Use the bundled binary or update dynare.serverPath.");
    const instance = service.currentInstance, version = document.version;
    let stale = false;
    const subscriptions = [
      vscode.workspace.onDidChangeTextDocument(event => { if (event.contentChanges.length) stale = true; }),
      vscode.workspace.onDidCloseTextDocument(() => { stale = true; }),
      vscode.workspace.onDidChangeWorkspaceFolders(() => { stale = true; }),
      vscode.workspace.onDidChangeConfiguration(event => { if (event.affectsConfiguration("dynare")) stale = true; }),
      service.onDidInvalidate(event => { if (event.reason === "file" || event.reason === "lifecycle") stale = true; }),
    ];
    const current = (): boolean => !stale && !token.isCancellationRequested && client === service.client
      && instance === service.currentInstance && document.version === version;
    const params = { textDocument: { uri: document.uri.toString() }, position: { line: position.line, character: position.character }, newName };
    try {
      const offered = capture(await service.execute("dynare/captureRename", [params], token));
      if (!offered || !current()) refuse("The Rename capture or current source context could not be verified.");
      const loaded = new Map<string, vscode.TextDocument>();
      for (const source of offered.sources) {
        const uri = vscode.Uri.parse(source.uri);
        const key = sourceUriKey(uri);
        const open = vscode.workspace.textDocuments.find(candidate => sourceUriKey(candidate.uri) === key);
        if (source.version !== null && (!open || open.version !== source.version)) refuse("An open Rename source has changed version. Request Rename again.");
        const target = open ?? await vscode.workspace.openTextDocument(uri);
        if (!current() || sourceUriKey(target.uri) !== key || target.getText() !== source.text) refuse("Rename source text or model inputs changed while loading. Request Rename again.");
        loaded.set(key, target);
      }
      const versions = new Map([...loaded].map(([uri, target]) => [uri, target.version]));
      // didOpen can replace a saved input with an identical open overlay. A
      // second complete decision proves its binding and source after loading.
      const checked = capture(await service.execute("dynare/captureRename", [params], token));
      if (!checked || !current() || JSON.stringify(offered.owners) !== JSON.stringify(checked.owners)
        || offered.sources.length !== checked.sources.length) refuse("Rename owners or captured sources changed while loading. Request Rename again.");
      for (const source of checked.sources) {
        const key = identity(source.uri);
        const before = offered.sources.find(row => identity(row.uri) === key);
        const target = loaded.get(key);
        if (!before || before.text !== source.text || !target || target.version !== versions.get(key)
          || target.getText() !== source.text || source.version !== null && source.version !== target.version) refuse("A captured Rename source changed while loading. Request Rename again.");
      }
      const shape = (edit: ProtocolEdit): string => JSON.stringify(edit.documentChanges?.map(change => TextDocumentEdit.is(change)
        ? { uri: identity(change.textDocument.uri), edits: change.edits } : change).sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right))));
      if (shape(offered.edit) !== shape(checked.edit)) refuse("The engine changed the Rename edit between captures. Request Rename again.");
      const edit = await client.protocol2CodeConverter.asWorkspaceEdit(checked.edit, token);
      if (!current() || [...loaded].some(([uri, target]) => target.version !== versions.get(uri))) refuse("Rename inputs changed during edit conversion. Request Rename again.");
      if (!edit) refuse("The engine could not convert the complete Rename edit.");
      // A source can decide a binding/collision without containing an edit.
      // Empty insertions include its version in the public application request;
      // the native text-edit worker discards the no-op after version validation.
      for (const [uri, target] of loaded) {
        if (!checked.edit.documentChanges?.some(change => TextDocumentEdit.is(change) && identity(change.textDocument.uri) === uri)) {
          edit.insert(target.uri, new vscode.Position(0, 0), "");
        }
      }
      // Public workspace.applyEdit attaches the currently loaded versions.
      // The native RenameProvider route drops those versions and is disabled.
      return await consume(edit);
    } finally { for (const subscription of subscriptions) subscription.dispose(); }
}

export async function renameSymbol(service: DygnosisClient): Promise<boolean> {
  try {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "dynare" || !["file", "untitled"].includes(editor.document.uri.scheme)) {
    refuse("Rename requires an open Dynare source file or untitled Dynare document.");
  }
  const document = editor.document, position = editor.selection.active, version = document.version;
  await service.ensureStarted();
  const client = service.client, instance = service.currentInstance;
  if (!client?.initializeResult?.capabilities.executeCommandProvider?.commands.includes("dynare/captureRename")) {
    refuse("This engine does not support safe Rename. Use the bundled binary or update dynare.serverPath.");
  }
  const cancellation = new vscode.CancellationTokenSource();
  try {
    const prepared = await client.sendRequest(PrepareRenameRequest.type, {
      textDocument: { uri: document.uri.toString() }, position: { line: position.line, character: position.character },
    }, cancellation.token);
    if (!prepared) { refuse("The engine could not prepare Rename at this position. Place the cursor on a declared model name."); }
    if (document.version !== version || client !== service.client || instance !== service.currentInstance) {
      refuse("The document or server changed during Rename. Try again.");
    }
    const range = "range" in prepared ? prepared.range : "start" in prepared ? prepared : undefined;
    if (!range) throw new Error("The engine returned an invalid Rename range. Update the engine and try again.");
    const name = document.getText(client.protocol2CodeConverter.asRange(range));
    const newName = await vscode.window.showInputBox({ title: "Dygnosis: Rename symbol", prompt: `New name for ${name}`, value: name });
    if (newName === undefined || newName === name) return false;
    if (!newName.trim()) { refuse("Rename requires a nonempty identifier."); }
    if (document.version !== version || client !== service.client || instance !== service.currentInstance) {
      refuse("The document or server changed while entering the name. Try again.");
    }
    const applied = await withCheckedRenameEdit(service, document, position, newName, cancellation.token,
      edit => vscode.workspace.applyEdit(edit, { isRefactoring: true }));
    if (applied === false) refuse("VS Code did not apply Rename. A target may have changed or become read-only. Refresh the model and try again.");
    return applied === true;
  } finally { cancellation.dispose(); }
  } catch (error) { await explainFailure(service, error); return false; }
}
