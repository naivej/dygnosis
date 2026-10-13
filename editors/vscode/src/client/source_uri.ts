import * as vscode from "vscode";

/** Match native document identity, including Windows drive-letter URI aliases. */
export function sourceUriKey(uri: vscode.Uri): string {
  return uri.scheme === "file" && process.platform === "win32"
    ? JSON.stringify([uri.scheme, uri.authority, uri.fsPath.toLowerCase(), uri.query, uri.fragment]) : uri.toString();
}

export function sameSourceUri(left: vscode.Uri, right: vscode.Uri): boolean {
  return left.scheme === right.scheme && sourceUriKey(left) === sourceUriKey(right);
}
