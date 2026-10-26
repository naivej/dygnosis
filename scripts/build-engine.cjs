// Build both workspace engines and use Cargo's reported executables, including custom target folders.
const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const path = require('node:path');

function selectBinaries(artifacts) {
  function executable(name) {
    return artifacts.find(item => item.reason === 'compiler-artifact'
      && item.target.name === name && item.target.kind.includes('bin') && item.executable)?.executable;
  }
  const lsp = executable('dygnosis');
  const mcp = executable('dygnosis-mcp');
  assert.ok(lsp, "Cargo did not report the dygnosis LSP executable.");
  assert.ok(mcp, "Cargo did not report the dygnosis-mcp executable.");
  assert.notEqual(lsp, mcp, 'Cargo must report separate LSP and MCP executables.');
  return { lsp, mcp };
}

function buildEngine(root = path.resolve(__dirname, '..'), { release = false, target, env = process.env } = {}) {
  const args = ['build', '--locked', '--bin', 'dygnosis', '--bin', 'dygnosis-mcp', '--message-format=json'];
  if (release) args.push('--release');
  if (target) args.push('--target', target);
  let output;
  try {
    output = childProcess.execFileSync('cargo', args, {
      cwd: root, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true,
      timeout: 20 * 60 * 1000, maxBuffer: 32 * 1024 * 1024,
    });
  } catch (error) {
    throw new Error(`Could not build the workspace engine: ${error.message}`, { cause: error });
  }
  const artifacts = output.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  return selectBinaries(artifacts);
}

module.exports = { buildEngine, selectBinaries };
