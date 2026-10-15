const assert = require('node:assert/strict');
const test = require('node:test');
const { canonicalMcpResult, mcpProbeCases } = require('../scripts/common.cjs');
const { probeNativeMcp } = require('../scripts/native-mcp-host.cjs');

test('package probes read contract 3 from structured or text results and reject legacy envelopes', () => {
  const result = { schema_version: 3, data: { summary: {} } };
  assert.equal(canonicalMcpResult({ structuredContent: result }), result);
  assert.deepEqual(canonicalMcpResult({ content: [
    { type: 'text', text: 'Explanation' }, { type: 'text', text: JSON.stringify(result) },
  ] }), result);
  assert.throws(() => canonicalMcpResult({ structuredContent: { schema_version: 2 } }), /contract 3/);
  assert.throws(() => canonicalMcpResult({ content: [{ type: 'text', text: '{"schema_version":2}' }] }), /schema_version 3/);
});

// This fake workbench checks native routing and guarded inputs. Real stdio and
// native-host acceptance separately check the engine and workbench provider.
test('native probes reuse a binding through Symbol Info and guarded Rename, then stop their server', async () => {
  const invocations = [], commands = [], token = {}, revision = 'captured-revision';
  let cancelled = false, disposed = false;
  class TextPart { constructor(value) { this.value = value; } }
  const tools = Object.keys(mcpProbeCases).map(name => ({ name: `native_${name}` }));
  const vscode = {
    CancellationTokenSource: class {
      token = token;
      cancel() { cancelled = true; }
      dispose() { disposed = true; }
    },
    LanguageModelTextPart: TextPart,
    workspace: { isTrusted: true },
    commands: {
      getCommands: async () => ['workbench.mcp.startServer'],
      executeCommand: async (...args) => { commands.push(args); },
    },
    lm: {
      tools,
      invokeTool: async (name, { input }, cancellationToken) => {
        assert.equal(cancellationToken, token);
        invocations.push({ name, input });
        const result = { schema_version: 3, outcome: { status: 'ok' }, data: {}, inputs: [{ revision }] };
        if (name.endsWith('_model_info')) result.data.structure = { n_equations: 1, n_endogenous: 1 };
        if (name.endsWith('_symbol_info')) {
          assert.ok(['name', 'binding'].includes(input.symbol.kind));
          if (input.symbol.kind === 'binding') {
            assert.equal(input.symbol.binding_id, 'binding-y');
            assert.equal(input.expected_revision, revision);
            assert.deepEqual(input.sections, ['references']);
          }
          result.data = { binding: { name: 'y', binding_id: 'binding-y' }, references: [{ role: 'use' }] };
        }
        if (name.endsWith('_rename')) {
          if (input.new_name === 'z') {
            assert.deepEqual(input.symbol, { kind: 'binding', binding_id: 'binding-y' });
            assert.equal(input.expected_revision, revision);
          }
          result.outcome.status = 'changed';
          result.proposal = { revision, edits: [{ new_text: input.new_name }] };
        }
        return { content: [new TextPart(JSON.stringify(result))] };
      },
    },
  };
  const evidence = await probeNativeMcp(vscode, 'CoconutWater.dygnosis');
  assert.deepEqual(evidence.invoked_tools, Object.keys(mcpProbeCases));
  assert.equal(evidence.semantic_symbol, 'y');
  assert.equal(evidence.semantic_reference_count, 1);
  assert.equal(evidence.guarded_rename_edits, 1);
  assert.ok(invocations.some(({ input }) => input.sections?.includes('references') && input.symbol.kind === 'binding'));
  assert.equal(commands[0][0], 'workbench.mcp.startServer');
  assert.deepEqual(commands.at(-1), ['workbench.mcp.stopServer', 'coconutwater.dygnosis/Dygnosis']);
  assert.ok(cancelled && disposed);
});
