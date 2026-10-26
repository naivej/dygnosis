const assert = require("node:assert/strict");
const test = require("node:test");
const { selectBinaries } = require("../../../scripts/build-engine.cjs");

test("engine build selection requires Cargo-reported LSP and MCP sibling artifacts", () => {
  const artifacts = [
    { reason: "compiler-artifact", target: { name: "dygnosis", kind: ["lib"] }, executable: "wrong-library" },
    { reason: "compiler-artifact", target: { name: "other", kind: ["bin"] }, executable: "wrong-binary" },
    { reason: "compiler-artifact", target: { name: "dygnosis", kind: ["bin"] }, executable: "target/custom/dygnosis.exe" },
    { reason: "compiler-artifact", target: { name: "dygnosis-mcp", kind: ["bin"] }, executable: "target/custom/dygnosis-mcp.exe" },
  ];
  assert.deepEqual(selectBinaries(artifacts), {
    lsp: "target/custom/dygnosis.exe",
    mcp: "target/custom/dygnosis-mcp.exe",
  });
  // Prevent staging one executable twice or packaging a partial Cargo build.
  assert.throws(() => selectBinaries(artifacts.filter(item => item.target.name !== "dygnosis-mcp")), /did not report the dygnosis-mcp/);
  assert.throws(() => selectBinaries(artifacts.map(item => ({ ...item, executable: "same-path" }))), /separate LSP and MCP/);
});
