import { sep } from "node:path";
import type * as vscode from "vscode";

/** Format a file path for the current host's UI. */
export function displayPath(path: string): string {
  return sep === "\\" ? path.replaceAll("/", sep) : path;
}

export function displaySourcePath(uri: vscode.Uri): string {
  if (uri.scheme === "file") return uri.fsPath;
  return uri.scheme === "untitled" ? uri.path : uri.toString();
}

/** Match native document identity, including Windows drive-letter URI aliases. */
export function sourceUriKey(uri: vscode.Uri): string {
  return uri.scheme === "file" && process.platform === "win32"
    ? JSON.stringify([uri.scheme, uri.authority, uri.fsPath.toLowerCase(), uri.query, uri.fragment]) : uri.toString();
}

export function sameSourceUri(left: vscode.Uri, right: vscode.Uri): boolean {
  return left.scheme === right.scheme && sourceUriKey(left) === sourceUriKey(right);
}
