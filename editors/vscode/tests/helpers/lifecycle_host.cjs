// Verify native restart with a real document-link provider request in flight.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { performance } = require("node:perf_hooks");
const vscode = require("vscode");

exports.checkLifecycle = async function checkLifecycle(service, workspaceRoot, evidence, { waitFor }) {
  const root = path.join(workspaceRoot, "restart.mod"), include = path.join(workspaceRoot, "restart.inc");
  await fs.writeFile(include, "parameters p; p=.5;\n");
  await fs.writeFile(root, "@#include \"restart.inc\"\nvar y; model; y=p; end;\n");
  const document = await vscode.workspace.openTextDocument(vscode.Uri.file(root));
  await vscode.window.showTextDocument(document);
  await waitFor(async () => (await service.modelInfo(document.uri))?.complete, "restart fixture is ready");
  const output = service.output, originalAppend = output.append, originalAppendLine = output.appendLine, messages = [];
  output.append = text => { messages.push(text); originalAppend.call(output, text); };
  output.appendLine = text => { messages.push(text); originalAppendLine.call(output, text); };
  const durations = [];
  try {
    for (let iteration = 0; iteration < 4; ++iteration) {
      const previous = service.client, instance = service.currentInstance;
      const provider = previous.getFeature("textDocument/documentLink").getProvider(document);
      assert.ok(provider, "the native link provider must be registered");
      const cancellation = new vscode.CancellationTokenSource();
      try {
        const links = provider.provideDocumentLinks(document, cancellation.token);
        const started = performance.now();
        await service.restart();
        durations.push(Math.round(performance.now() - started));
        await links;
        assert.notEqual(service.client, previous);
        assert.ok(service.currentInstance > instance);
        await waitFor(async () => (await service.modelInfo(document.uri))?.complete, "replacement answers model requests");
      } finally { cancellation.cancel(); cancellation.dispose(); }
    }
    await service.shutdown();
    assert.equal(service.client, undefined);
    assert.doesNotMatch(messages.join("\n"), /EPIPE|stream was destroyed|Request textDocument\/documentLink failed|Stopping server failed|Connection to server got closed/i);
    evidence.lifecycle = { restarts: durations.length, restart_ms: durations, document_link_in_flight: true, transport_errors: 0 };
    evidence.checks.push("native restart/shutdown with document-link requests, replacement model replies and no closed-stream errors");
  } finally {
    output.append = originalAppend; output.appendLine = originalAppendLine;
  }
};
