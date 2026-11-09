import * as vscode from "vscode";
import type { Position, Range } from "./protocol";
import { refuse } from "./refusal";
import { sameSourceUri } from "./source_uri";

/** Text-menu commands receive a URI. Keyboard and palette commands use the active editor. */
export function commandEditor(resource?: unknown): vscode.TextEditor | undefined {
  const editor = vscode.window.activeTextEditor;
  if (resource !== undefined && (!editor || !(resource instanceof vscode.Uri) || !sameSourceUri(editor.document.uri, resource))) {
    refuse("The editor changed. Open the context menu in the file you want to use and try again.");
  }
  return editor;
}

export function captureSelection(editor: vscode.TextEditor, target: string): Range {
  if (editor.selections?.length > 1) refuse(`Select one ${target} and try again.`);
  const { active, start = active, end = active } = editor.selection;
  return { start: { line: start.line, character: start.character }, end: { line: end.line, character: end.character } };
}

export function comparePositions(left: Position, right: Position): number {
  return left.line - right.line || left.character - right.character;
}

/** The last position in an LSP range belongs to the following text. */
export function containsSelection(range: Range, selection: Range): boolean {
  return comparePositions(range.start, selection.start) <= 0 && comparePositions(selection.start, range.end) < 0 &&
    comparePositions(selection.end, range.end) <= 0;
}
