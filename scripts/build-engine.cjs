// Build the workspace engine and use Cargo's reported executable, including custom target folders.
const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const path = require('node:path');

function buildEngine(root = path.resolve(__dirname, '..')) {
  let output;
  try {
    output = childProcess.execFileSync('cargo', ['build', '--locked', '--bin', 'dygnosis', '--message-format=json'], {
      cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true,
      timeout: 20 * 60 * 1000, maxBuffer: 32 * 1024 * 1024,
    });
  } catch (error) {
    throw new Error(`Could not build the workspace engine: ${error.message}`, { cause: error });
  }
  const artifacts = output.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  const executable = artifacts.find(item => item.reason === 'compiler-artifact'
    && item.target.name === 'dygnosis' && item.target.kind.includes('bin') && item.executable)?.executable;
  assert.ok(executable, 'Cargo did not report the dygnosis executable; Help was not refreshed.');
  return executable;
}

module.exports = { buildEngine };
