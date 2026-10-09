// Replay MCP discovery through the JSON validator bundled with a VS Code host.
// Usage: node tests/native_mcp_schema.cjs <dygnosis binary> <VS Code resources/app>
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const vm = require('node:vm');

async function listTools(binary) {
  const child = spawn(binary, ['mcp'], { stdio: ['pipe', 'pipe', 'pipe'] });
  child.stdout.setEncoding('utf8');
  let buffer = '';
  const pending = new Map();
  let id = 0;
  const rejectPending = error => {
    for (const request of pending.values()) request.reject(error);
    pending.clear();
  };
  child.on('error', rejectPending);
  child.on('exit', code => rejectPending(new Error(`MCP process exited with code ${code}`)));
  child.stdout.on('data', chunk => {
    buffer += chunk;
    let end;
    while ((end = buffer.indexOf('\n')) >= 0) {
      let message;
      try { message = JSON.parse(buffer.slice(0, end)); }
      catch (error) { rejectPending(error); child.kill(); return; }
      buffer = buffer.slice(end + 1);
      if (pending.has(message.id)) {
        const request = pending.get(message.id);
        pending.delete(message.id);
        request.resolve(message);
      }
    }
  });
  const request = (method, params) => new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => {
      pending.delete(requestId);
      reject(new Error(`${method} timed out`));
    }, 10000);
    pending.set(requestId, {
      resolve: message => { clearTimeout(timer); resolve(message); },
      reject: error => { clearTimeout(timer); reject(error); },
    });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id: requestId, method, params })}\n`);
  });
  try {
    const initialized = await request('initialize', {
      protocolVersion: '2025-03-26', capabilities: {},
      clientInfo: { name: 'native-schema-regression', version: '1' },
    });
    assert.ok(initialized.result, JSON.stringify(initialized));
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
    const listed = await request('tools/list', {});
    assert.ok(listed.result, JSON.stringify(listed));
    return listed.result.tools;
  } finally {
    child.kill();
  }
}

async function bundledValidator(appPath) {
  assert.equal(JSON.parse(fs.readFileSync(path.join(appPath, 'package.json'), 'utf8')).version, '1.102.0',
    'This bundled-validator replay is pinned to the minimum VS Code host');
  const main = path.join(appPath, 'extensions/json-language-features/server/dist/node/jsonServerMain.js');
  // Expose webpack's loader while skipping the language server entry point.
  // All validator and schema code still comes from the installed VS Code build.
  const source = fs.readFileSync(main, 'utf8');
  const marker = 'var s=i(5474)';
  assert.ok(source.includes(marker), 'VS Code JSON server bundle layout changed');
  const context = {
    require: createRequire(main), exports: {}, process,
    console, URL, fetch, setTimeout, clearTimeout,
  };
  vm.runInNewContext(source.replace(marker, 'globalThis.bundleRequire=i;var s={}'), context);
  await context.bundleRequire.e(875);
  const api = context.bundleRequire(7547);
  const requestedUris = [];
  const service = api.getLanguageService({
    workspaceContext: { resolveRelativePath: (relative, base) => new URL(relative, base).toString() },
    schemaRequestService: async uri => {
      requestedUris.push(uri);
      throw new Error(`Unexpected schema request: ${uri}`);
    },
  });
  service.configure({ schemas: [{ uri: 'https://json-schema.org/draft-07/schema', fileMatch: ['test://mcp-input.json'] }] });
  const validate = async schema => {
    const document = api.TextDocument.create('test://mcp-input.json', 'json', 1, JSON.stringify(schema));
    return service.doValidation(document, service.parseJSONDocument(document), {});
  };
  return { validate, requestedUris };
}

async function main() {
  const [binary, appPath] = process.argv.slice(2);
  assert.ok(binary && appPath, 'Supply the dygnosis binary and VS Code resources/app directory');
  const tools = await listTools(binary);
  assert.equal(tools.length, 15, 'all public tools are discoverable over stdio');
  const { validate, requestedUris } = await bundledValidator(appPath);
  // Verify the replay reaches schema validation, rather than silently accepting
  // all inputs if the bundle or schema-association seam changes.
  assert.equal((await validate({ type: 'object', properties: { value: { type: ['string', 'null'] } } })).length, 0);
  assert.ok((await validate({ type: 'invalid-json-type' })).length > 0);
  const omitted = [];
  for (const tool of tools) {
    const errors = await validate(tool.inputSchema);
    if (errors.length) omitted.push({ name: tool.name, errors: errors.map(error => ({ code: error.code, message: error.message })) });
  }
  console.log(JSON.stringify({ discovered: tools.length, omitted }, null, 2));
  assert.equal(omitted.length, 0, 'native VS Code must register every discovered MCP tool');
  assert.deepEqual(requestedUris, [], 'fixed input schemas need no external metadata');
}

main().catch(error => { console.error(error); process.exitCode = 1; });
