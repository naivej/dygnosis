// Replay MCP discovery through the JSON validator bundled with a VS Code host.
// Usage: node tests/native_mcp_schema.cjs <binary> <VS Code resources/app> [expected host version]
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const vm = require('node:vm');

async function listTools(binary) {
  const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'pipe'] });
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

async function bundledValidator(appPath, expectedVersion) {
  const hostVersion = JSON.parse(fs.readFileSync(path.join(appPath, 'package.json'), 'utf8')).version;
  assert.equal(hostVersion, expectedVersion, 'Unexpected VS Code host version');
  const main = path.join(appPath, 'extensions/json-language-features/server/dist/node/jsonServerMain.js');
  const source = fs.readFileSync(main, 'utf8');
  const context = {
    require: createRequire(main), exports: {}, process,
    console, URL, fetch, setTimeout, clearTimeout,
  };
  let api;
  const webpackMarker = 'var s=i(5474)';
  if (source.includes(webpackMarker)) {
    // Expose webpack's loader while skipping the language server entry point.
    // All validator and schema code still comes from the installed VS Code build.
    vm.runInNewContext(source.replace(webpackMarker, 'globalThis.bundleRequire=i;var s={}'), context);
    await context.bundleRequire.e(875);
    api = context.bundleRequire(7547);
  } else {
    // Newer hosts bundle the JSON server as ESM with lazy CJS-style modules.
    // Expose only the validator and document factory, and skip the server entry.
    const bootstrap = "import { createRequire } from 'module'; const require = createRequire(import.meta.url);";
    const entryPoint = 'T0();\n//# sourceMappingURL=jsonServerMain.js.map';
    const exportMarkers = ['function Hu(e)', 'var $m=ue(', 'ir=ue(()=>', 'ic||(ic={})'];
    assert.ok(source.startsWith(bootstrap), 'VS Code JSON server ESM bootstrap layout changed');
    assert.ok(source.includes(entryPoint), 'VS Code JSON server ESM entry point changed');
    for (const marker of exportMarkers) assert.ok(source.includes(marker), `VS Code JSON server validator export seam changed: ${marker}`);
    const testEntry = 'ir();$m();globalThis.nativeValidator={getLanguageService:Hu,TextDocument:ic};';
    const fsImport = 'import{promises as b0}from"fs";';
    assert.ok(source.includes(fsImport), 'VS Code JSON server filesystem import changed');
    const testSource = source.replace(bootstrap, '').replace(fsImport, 'const b0=require("fs").promises;').replace(entryPoint, testEntry);
    vm.runInNewContext(testSource, context);
    api = context.nativeValidator;
    assert.ok(api && typeof api.getLanguageService === 'function' && typeof api.TextDocument?.create === 'function',
      `VS Code JSON server native validator exports are unavailable (factory=${typeof api?.getLanguageService}, document=${typeof api?.TextDocument}, create=${typeof api?.TextDocument?.create})`);
  }
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
  return { validate, requestedUris, hostVersion };
}

async function main() {
  const [binary, appPath, expectedVersion = '1.102.0'] = process.argv.slice(2);
  assert.ok(binary && appPath, 'Supply the dygnosis binary and VS Code resources/app directory');
  const tools = await listTools(binary);
  assert.ok(Array.isArray(tools) && tools.length > 0, 'public tools are discoverable over stdio');
  for (const name of ['dynare_symbol_info', 'dynare_actions']) {
    const tool = tools.find(candidate => candidate.name === name);
    assert.ok(tool, `${name} metadata is discoverable`);
    assert.equal(tool.inputSchema.type, 'object', `${name} exposes an object input schema`);
    assert.ok(tool.inputSchema.properties && Object.keys(tool.inputSchema.properties).length > 0,
      `${name} declares input controls`);
  }
  const { validate, requestedUris, hostVersion } = await bundledValidator(appPath, expectedVersion);
  // Verify the replay reaches schema validation, rather than silently accepting
  // all inputs if the bundle or schema-association seam changes.
  assert.equal((await validate({ type: 'object', properties: { value: { type: ['string', 'null'] } } })).length, 0);
  assert.ok((await validate({ type: 'invalid-json-type' })).length > 0);
  const omitted = [];
  for (const tool of tools) {
    const errors = await validate(tool.inputSchema);
    if (errors.length) omitted.push({ name: tool.name, errors: errors.map(error => ({ code: error.code, message: error.message })) });
  }
  console.log(JSON.stringify({ hostVersion, discovered: tools.length, omitted }, null, 2));
  assert.equal(omitted.length, 0, 'native VS Code must register every discovered MCP tool');
  assert.deepEqual(requestedUris, [], 'fixed input schemas need no external metadata');
}

main().catch(error => { console.error(error); process.exitCode = 1; });
