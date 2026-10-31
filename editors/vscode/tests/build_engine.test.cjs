const assert = require("node:assert/strict");
const childProcess = require("node:child_process");
const test = require("node:test");
const { buildEngine, selectBinaries } = require("../../../scripts/build-engine.cjs");

function slowCargo(t) {
  const binaries = { lsp: "target/release/dygnosis", mcp: "target/release/dygnosis-mcp" };
  t.mock.method(childProcess, "execFileSync", (command, args, options) => {
    assert.equal(command, "cargo");
    assert.ok(Number.isFinite(options.timeout) && options.timeout > 0, "Cargo must retain a finite deadline");
    // Simulate cold native compilation without waiting 25 minutes in the test.
    if (options.timeout <= 25 * 60 * 1000) {
      throw Object.assign(new Error("spawnSync cargo ETIMEDOUT"), { code: "ETIMEDOUT" });
    }
    assert.ok(args.includes("--release"));
    return [
      { reason: "compiler-artifact", target: { name: "dygnosis", kind: ["bin"] }, executable: binaries.lsp },
      { reason: "compiler-artifact", target: { name: "dygnosis-mcp", kind: ["bin"] }, executable: binaries.mcp },
    ].map(item => JSON.stringify(item)).join("\n");
  });
  return binaries;
}

// Guards cold release compilation being killed by the development-build deadline.
test("release engine builds allow cold native compilation beyond twenty minutes", t => {
  const binaries = slowCargo(t);
  assert.deepEqual(buildEngine(undefined, { release: true, target: "x86_64-apple-darwin" }), binaries);
});

// Guards release-only headroom weakening the development deadline or hiding its cause.
test("development engine builds still report a timed-out Cargo process", t => {
  slowCargo(t);
  assert.throws(() => buildEngine(), error => error.cause?.code === "ETIMEDOUT"
    && /Could not build the workspace engine/.test(error.message));
});

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
