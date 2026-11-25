// Shared target, file, and process helpers for product packaging.
const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { createHash } = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");

const targets = {
  "win32-x64": { rust: "x86_64-pc-windows-msvc", platform: "win32", arch: "x64", vscode: "win32-x64-archive" },
  "win32-arm64": { rust: "aarch64-pc-windows-msvc", platform: "win32", arch: "arm64", vscode: "win32-arm64-archive" },
  "darwin-x64": { rust: "x86_64-apple-darwin", platform: "darwin", arch: "x64", vscode: "darwin" },
  "darwin-arm64": { rust: "aarch64-apple-darwin", platform: "darwin", arch: "arm64", vscode: "darwin-arm64" },
  "linux-x64": { rust: "x86_64-unknown-linux-gnu", platform: "linux", arch: "x64", vscode: "linux-x64" },
  "linux-arm64": { rust: "aarch64-unknown-linux-gnu", platform: "linux", arch: "arm64", vscode: "linux-arm64" },
};

function targetInfo(target) {
  assert.ok(Object.hasOwn(targets, target), `Unsupported target: ${target}`);
  return targets[target];
}

function execute(file, args, options = {}) {
  if (file === "tar" && process.platform === "win32") {
    assert.ok(process.env.SystemRoot, "Windows archive checks require SystemRoot");
    file = path.join(process.env.SystemRoot, "System32", "tar.exe");
  }
  return execFileSync(file, args, { encoding: "utf8", windowsHide: true, timeout: 120000, maxBuffer: 32 * 1024 * 1024, ...options });
}

async function sha256(file) {
  return createHash("sha256").update(await fs.readFile(file)).digest("hex");
}

async function writeJson(file, value) {
  await fs.writeFile(file, JSON.stringify(value, null, 2) + "\n");
}

function binaryName(target) {
  return targetInfo(target).platform === "win32" ? "dygnosis.exe" : "dygnosis";
}

function mcpBinaryName(target) {
  return targetInfo(target).platform === "win32" ? "dygnosis-mcp.exe" : "dygnosis-mcp";
}

module.exports = { targets, targetInfo, execute, sha256, writeJson, binaryName, mcpBinaryName };
