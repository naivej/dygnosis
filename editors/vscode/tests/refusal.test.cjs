// Verify that helper failures stay silent until the explicit command boundary.
const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const messages = [];
const original = Module._load;
Module._load = function (id, ...args) {
  return id === "vscode" ? { window: { showInformationMessage: async message => { messages.push(message); } } } : original.call(this, id, ...args);
};
const { CommandCancelled, refuse, runCommand } = require(compiledModule("refusal"));
Module._load = original;

test("nested helpers preserve the cause and one command notification", async () => {
  messages.length = 0;
  const logs = [], service = { log: message => logs.push(message) };
  const helper = async () => refuse("The included source is read-only.");
  await assert.rejects(helper(), /read-only/);
  assert.deepEqual(messages, []);
  await runCommand(service, () => helper());
  assert.deepEqual(messages, ["The included source is read-only."]);
  assert.equal(logs.length, 1);
});

test("inactive and cancelled commands do not show a refusal notification", async () => {
  messages.length = 0;
  const service = { log() {} };
  await runCommand(service, () => refuse("Disposed"), () => false);
  await runCommand(service, () => undefined);
  await runCommand(service, () => { throw new CommandCancelled(); });
  assert.deepEqual(messages, []);
});
