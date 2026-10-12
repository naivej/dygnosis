import * as vscode from "vscode";
import { GitAPI, GitSourceError, GitSources } from "./git_source";

interface GitExtension { enabled: boolean; getAPI(version: 1): GitAPI }

/** Git acquisition is optional; native analysis can activate without vscode.git. */
export async function createGitSources(): Promise<GitSources> {
  const extension = vscode.extensions.getExtension<GitExtension>("vscode.git");
  if (!extension) throw new GitSourceError("git_unavailable", "The built-in Git extension is unavailable. Enable Git support to compare revisions.");
  const exports = extension.isActive ? extension.exports : await extension.activate();
  if (!exports.enabled) throw new GitSourceError("git_unavailable", "The built-in Git extension is disabled. Enable Git support to compare revisions.");
  try { return new GitSources(exports.getAPI(1), file => vscode.Uri.file(file)); }
  catch (error) {
    if (error instanceof GitSourceError) throw error;
    throw new GitSourceError("git_unavailable", "The built-in Git extension cannot provide a compatible repository interface.");
  }
}
