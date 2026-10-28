const assert = require('node:assert/strict');
const test = require('node:test');
const { canonicalMcpResult, mcpProbeCases, rpc } = require('../scripts/common.cjs');
const { probeNativeMcp } = require('../scripts/native-mcp-host.cjs');

test('package probes accept current MCP and VS Code tool results', () => {
  // Guards shared decoding for MCP structured data and VS Code's one JSON part.
  const result = { schema_version: 3, data: { summary: {} } };
  assert.equal(canonicalMcpResult({ structuredContent: result }), result);
  assert.deepEqual(canonicalMcpResult({ content: [{ type: 'text', text: JSON.stringify(result) }] }), result);
  assert.throws(() => canonicalMcpResult({ structuredContent: { schema_version: 2 } }), /contract 3/);
  assert.throws(() => canonicalMcpResult({ content: [{ type: 'text', text: 'extra' }, { type: 'text', text: JSON.stringify(result) }] }), /one canonical/);
});

test('stdio probe preserves a child reuse override and captures MCP retention trace from stderr', async () => {
  const source = `let buffer=''; process.stdin.on('data', chunk => { buffer += chunk; let end; while ((end=buffer.indexOf('\\n')) >= 0) { const line=buffer.slice(0,end); buffer=buffer.slice(end+1); if (!line) continue; const request=JSON.parse(line); process.stderr.write('MCP_RETENTION '+JSON.stringify({event:'complete',cache_enabled:false})+'\\n'); process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,result:{reuse:process.env.DYGNOSIS_MCP_REUSE}})+'\\n'); } });`;
  const session = rpc(process.execPath, ['-e', source], false, { env: { ...process.env, DYGNOSIS_MCP_REUSE: '0' } });
  try {
    const result = await session.request('probe', {});
    assert.equal(result.reuse, '0');
    for (let attempt = 0; attempt < 50 && session.retentionEvents().length === 0; ++attempt) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    assert.deepEqual(session.retentionEvents(), [{ event: 'complete', cache_enabled: false }]);
  } finally { session.close(); }
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
