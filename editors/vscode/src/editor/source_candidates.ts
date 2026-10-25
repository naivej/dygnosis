import * as vscode from "vscode";
import type { DygnosisClient } from "../client/client";
import { location, record } from "../client/protocol";
import type { Location } from "../client/protocol";
import { CommandCancelled, refuse, runCommand } from "../client/refusal";

export interface SourceCandidate {
  source_id: string;
  name: string;
  kind?: string | null;
  label: "source candidate · unverified";
  availability: "active" | "inactive" | "mixed" | "unknown";
  location: Location;
}
export interface SourceCandidates {
  schema_version: 1;
  document_uri: string;
  document_version: number;
  source_revision: string;
  candidates: SourceCandidate[];
  message: string;
}

/** Source identity proves written bytes only; it never proves a semantic binding. */
export function parseSourceCandidates(value: unknown, document: vscode.TextDocument): SourceCandidates {
  if (record(value) && typeof value.error === "string") throw new Error(value.error);
  if (!record(value) || value.schema_version !== 1 || value.document_uri !== document.uri.toString() ||
      value.document_version !== document.version || typeof value.source_revision !== "string" || !value.source_revision ||
      typeof value.message !== "string" || !Array.isArray(value.candidates) || !value.candidates.every((candidate: unknown) => {
        if (!record(candidate) || typeof candidate.source_id !== "string" || !candidate.source_id ||
            typeof candidate.name !== "string" || !candidate.name || candidate.label !== "source candidate · unverified" ||
            !(candidate.kind === undefined || candidate.kind === null || typeof candidate.kind === "string") ||
            !["active", "inactive", "mixed", "unknown"].includes(String(candidate.availability)) || !location(candidate.location) ||
            candidate.location.uri !== document.uri.toString()) return false;
        const range = candidate.location.range;
        return [range.start, range.end].every(position => position.line < document.lineCount && position.character <= document.lineAt(position.line).text.length) &&
          document.getText(new vscode.Range(range.start.line, range.start.character, range.end.line, range.end.character)) === candidate.name;
      })) {
    throw new Error("Written source candidates are stale or invalid. Request source candidates again.");
  }
  return value as unknown as SourceCandidates;
}

function sameCandidate(left: SourceCandidate, right: SourceCandidate): boolean {
  return left.source_id === right.source_id && left.name === right.name && left.location.uri === right.location.uri &&
    left.location.range.start.line === right.location.range.start.line && left.location.range.start.character === right.location.range.start.character &&
    left.location.range.end.line === right.location.range.end.line && left.location.range.end.character === right.location.range.end.character;
}

/** Always ask, even for one candidate, because this is unverified source navigation. */
export function registerSourceCandidates(service: Pick<DygnosisClient, "ensureStarted" | "currentInstance" | "execute" | "log" | "openLocation">): vscode.Disposable {
  let disposed = false;
  const registration = vscode.commands.registerCommand("dygnosis.goToSourceCandidate", () => runCommand(service, async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.isClosed || !["file", "untitled"].includes(editor.document.uri.scheme)) {
      refuse("Open the written Dynare source and place the cursor on a name before finding source candidates.");
    }
    const document = editor.document, version = document.version;
    await service.ensureStarted();
    const instance = service.currentInstance;
    const current = (): boolean => !disposed && !document.isClosed && document.version === version &&
      vscode.window.activeTextEditor === editor && service.currentInstance === instance;
    const args = [{ textDocument: { uri: document.uri.toString() }, position: { line: editor.selection.active.line, character: editor.selection.active.character } }];
    const response = await service.execute("dynare/sourceCandidates", args);
    if (!current()) refuse("The source or server changed. Request source candidates again.");
    const proof = parseSourceCandidates(response, document);
    if (!proof.candidates.length) refuse(proof.message);
    const pick = await vscode.window.showQuickPick(proof.candidates.map(candidate => ({
      label: candidate.name, description: `${candidate.kind ?? "declaration"} · ${candidate.availability === "mixed" ? "active in some expansions" : candidate.availability}`,
      detail: `${vscode.workspace.asRelativePath(document.uri)}:${String(candidate.location.range.start.line + 1)}`, candidate,
    })), { title: "Dynare: Source declarations", placeHolder: "Choose a declaration; this does not establish references" });
    if (!pick) throw new CommandCancelled();
    if (!current()) refuse("The source or server changed while choosing a candidate. Request source candidates again.");
    await service.openLocation(pick.candidate.location, undefined, async target => {
      if (!current() || target.uri.toString() !== document.uri.toString() || target.version !== version) return false;
      const fresh = parseSourceCandidates(await service.execute("dynare/sourceCandidates", args), document);
      if (!current() || fresh.source_revision !== proof.source_revision || !fresh.candidates.some(candidate => sameCandidate(candidate, pick.candidate))) return false;
      return { isCurrent: () => current() && !target.isClosed && target.version === version };
    });
  }, () => !disposed));
  return new vscode.Disposable(() => { disposed = true; registration.dispose(); });
}
