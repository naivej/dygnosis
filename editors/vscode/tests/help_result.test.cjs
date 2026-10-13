const assert = require('node:assert/strict');
const test = require('node:test');
const { resultData } = require('../scripts/help.cjs');

test('Help generation reads canonical data without joining legacy and canonical JSON documents', () => {
  const data = [{ code: 'E001' }];
  const result = {
    content: [
      { type: 'text', text: JSON.stringify(data) },
      { type: 'text', text: JSON.stringify({ schema_version: 2, data }) },
    ],
  };
  assert.deepEqual(resultData(result), data);
  assert.deepEqual(resultData({ structuredContent: { schema_version: 2, data } }), data);
});

test('Help generation rejects a result with no canonical schema-versioned object', () => {
  assert.throws(() => resultData({ content: [{ type: 'text', text: 'legacy Markdown' }] }), /canonical schema_version 2/);
});
