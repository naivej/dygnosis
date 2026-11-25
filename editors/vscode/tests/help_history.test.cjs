const assert = require('node:assert/strict');
const test = require('node:test');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');

function reader(saved, isNarrow = false) {
  class Element {
    constructor() { this.children = []; this.handlers = {}; this.dataset = {}; this.value = ''; this.scrollTop = 0; }
    addEventListener(name, run) { this.handlers[name] = run; }
    replaceChildren() { this.children = []; }
    append(...children) { this.children.push(...children); }
    querySelectorAll() { return this.children; }
    setAttribute() {}
    removeAttribute() {}
    focus() {}
    scrollIntoView() { this.scrolledIntoView = true; }
  }
  const controls = new Map(), messages = [], events = {};
  let state;
  const media = { matches: isNarrow, addEventListener: (_name, run) => { media.change = run; } };
  const window = { matchMedia: () => media, addEventListener: (name, run) => { events[name] = run; } };
  const document = { getElementById: id => {
    if (!controls.has(id)) controls.set(id, new Element());
    return controls.get(id);
  }, createElement: () => new Element(), addEventListener() {} };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../media/help.js'), 'utf8'), {
    document, window, requestAnimationFrame: run => run(), acquireVsCodeApi: () => ({
      getState: () => saved, setState: next => { state = JSON.parse(JSON.stringify(next)); }, postMessage: message => messages.push(message),
    }),
  });
  return { controls, media, messages, state: () => state, receive: data => events.message({ data }), back: () => controls.get('back').handlers.click() };
}
const topics = ['get-started', 'reference', 'diagnostics'].map(id => ({ id, title: id, html: id, text: id, body: id, keywords: [] }));

test('reading history preserves each active-engine explanation and edition across other topics and restore', () => {
  const live = reader(); live.receive({ type: 'bundle', topics });
  const explanation = { code: 'E999', markdown: '# Engine-specific text', engineVersion: '0.10.0' };
  const extra = { id: 'check:E999', title: 'E999', html: '<h1>Engine-specific text</h1>', edition: 'Explanation from engine 0.10.0', explanation };
  live.receive({ type: 'open', destination: 'check:E999', extra });
  live.receive({ type: 'open', destination: 'diagnostics' }); live.back();
  assert.equal(live.controls.get('article').innerHTML, extra.html);
  assert.equal(live.controls.get('edition').textContent, extra.edition);
  assert.equal(live.controls.get('permalink').disabled, true, 'an unknown active-engine code has no bundled topic link');
  const saved = live.state();
  assert.equal(saved.history[1].extra, undefined, 'persist source Markdown, never rendered HTML');
  const restored = reader(saved), historyDetails = saved.history.map(({ destination, explanation }) => ({ destination, extra: explanation ? extra : undefined }));
  assert.equal(restored.messages[0].history[1].explanation.markdown, explanation.markdown);
  restored.receive({ type: 'bundle', topics, historyDetails });
  assert.equal(restored.controls.get('article').innerHTML, extra.html);
  assert.equal(restored.controls.get('edition').textContent, extra.edition);
});

test('restoring the old Support topic applies its new destination and resets the old scroll position', () => {
  const restored = reader({ history: [{ destination: 'troubleshoot', scroll: 500 }], index: 0 });
  restored.receive({ type: 'bundle', topics: [...topics, { id: 'about', title: 'About and support', keywords: [], html: 'Merged topic', body: '' }],
    historyDetails: [{ destination: 'about#support' }] });
  assert.equal(restored.controls.get('article').innerHTML, 'Merged topic');
  assert.deepEqual(restored.state().history[0], { destination: 'about#support', scroll: 0 });
  assert.equal(restored.controls.get('support').scrolledIntoView, true);
});

test('reopening the same section link jumps to its heading and keeps one history entry', () => {
  const live = reader();
  live.receive({ type: 'bundle', topics });
  const request = { type: 'open', destination: 'reference#diagnostic-codes' };
  live.receive(request);
  const heading = live.controls.get('diagnostic-codes'), main = live.controls.get('main');
  assert.equal(heading.scrolledIntoView, true);
  heading.scrolledIntoView = false;
  main.scrollTop = 777;
  live.receive({ type: 'open', destination: 'diagnostics' });
  live.back();
  assert.equal(main.scrollTop, 777, 'Back restores the reading position');
  assert.equal(heading.scrolledIntoView, false);
  live.receive(request);
  assert.equal(heading.scrolledIntoView, true, 'an explicit section request jumps to its heading');
  assert.equal(live.state().index, 1);
  assert.equal(live.controls.get('forward').disabled, false);
});

test('Help ranks title and keyword matches ahead of earlier body results and counts all matches', () => {
  const page = (title, keywords, text) => ({ id: title, title, keywords, text, html: text, body: text.toLowerCase() });
  const bodyMatches = Array.from({ length: 51 }, (_, i) => page(`Topic ${i}`, [], 'Discuss shocks here.'));
  const live = reader();
  live.receive({ type: 'bundle', topics: [...topics, ...bodyMatches,
    page('Shock paths', ['shocks'], 'Inspect a path.'),
    page('shocks options', [], 'Inspect options.'),
  ] });
  const search = live.controls.get('search');
  search.value = 'shocks';
  search.handlers.input();
  const results = live.controls.get('result-list').children;
  assert.equal(results.length, 50);
  assert.deepEqual(results.slice(0, 3).map(result => result.children[0].textContent), ['shocks options', 'Shock paths', 'Topic 0']);
  assert.equal(live.controls.get('result-count').textContent, 'Showing 50 of 53 matching topics');
});

test('clearing and reopening Help search preserve the article scroll position', () => {
  const live = reader();
  live.receive({ type: 'bundle', topics });
  const main = live.controls.get('main'), search = live.controls.get('search'), clear = live.controls.get('clear');
  main.scrollTop = 500;
  search.value = 'missing topic';
  search.handlers.input();
  assert.equal(clear.hidden, false);
  assert.equal(live.controls.get('article').hidden, true);
  main.handlers.scroll();
  const restored = reader(live.state());
  restored.receive({ type: 'bundle', topics });
  restored.controls.get('clear').handlers.click();
  assert.equal(restored.controls.get('main').scrollTop, 500);
  assert.equal(restored.controls.get('article').hidden, false);
  assert.equal(restored.controls.get('clear').hidden, true);
});

// Guards against the topic list covering the article after selection or resize.
test('narrow topics close after selection and wide navigation returns on resize', () => {
  const live = reader(undefined, true); live.receive({ type: 'bundle', topics });
  const disclosure = live.controls.get('topics');
  assert.equal(disclosure.open, false); disclosure.open = true;
  live.controls.get('contents').children[2].handlers.click();
  assert.equal(disclosure.open, false);
  assert.equal(live.controls.get('article').innerHTML, 'diagnostics');
  live.media.matches = false; live.media.change(); assert.equal(disclosure.open, true);
  live.media.matches = true; live.media.change(); assert.equal(disclosure.open, false);
});
