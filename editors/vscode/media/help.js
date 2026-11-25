/* global acquireVsCodeApi, document, window, requestAnimationFrame */
(() => {
  'use strict';
  const api = acquireVsCodeApi();
  const saved = api.getState();
  const history = Array.isArray(saved?.history) ? saved.history.filter(entry => typeof entry?.destination === 'string').slice(-100).map(entry => ({ destination: entry.destination, scroll: Number.isFinite(entry.scroll) && entry.scroll >= 0 ? entry.scroll : 0, explanation: entry.explanation })) : [];
  const state = { history: history.length ? history : [{ destination: 'get-started', scroll: 0 }], index: Number.isInteger(saved?.index) && saved.index >= 0 && saved.index < history.length ? saved.index : 0, query: typeof saved?.query === 'string' ? saved.query.slice(0, 300) : '' };
  const controls = Object.fromEntries(['edition', 'back', 'forward', 'permalink', 'search', 'clear', 'contents', 'topics', 'main', 'breadcrumb', 'results', 'result-count', 'result-list', 'article', 'notice', 'enlarged', 'close-image'].map(id => [id, document.getElementById(id)]));
  let topics = [], searchable = [];
  const narrow = window.matchMedia('(max-width: 700px)');
  const updateTopicsLayout = () => { controls.topics.open = !narrow.matches; };
  updateTopicsLayout();
  narrow.addEventListener('change', updateTopicsLayout);
  const node = (tag, text) => { const result = document.createElement(tag); result.textContent = text; return result; };
  const remember = () => api.setState({ ...state, history: state.history.map(({ destination, scroll, explanation }) => ({ destination, scroll, explanation })) });
  const current = () => state.history[state.index];
  const saveScroll = () => { if (!controls.article.hidden) current().scroll = controls.main.scrollTop; };
  function restoreScroll(scroll = current().scroll) {
    const anchor = current().destination.split('#')[1];
    if (anchor && !scroll) document.getElementById(anchor)?.scrollIntoView();
    else controls.main.scrollTop = scroll;
  }

  function render(focus = false) {
    const id = current().destination.split('#')[0];
    const topic = current().extra ?? topics.find(topic => topic.id === id) ?? topics.find(topic => topic.id === 'reference');
    if (!topic) return;
    controls.edition.textContent = topic.edition ?? '';
    const parent = id.startsWith('tool:') ? 'agents' : id.includes(':') ? 'reference' : undefined;
    controls.breadcrumb.replaceChildren();
    if (parent) {
      const link = node('a', parent === 'agents' ? 'Work with an agent' : 'Reference');
      link.href = '#';
      link.dataset.link = `help:${parent}`;
      controls.breadcrumb.append(link);
    }
    controls.article.innerHTML = topic.html;
    controls.permalink.disabled = !topics.some(topic => topic.id === id);
    controls.permalink.title = controls.permalink.disabled ? 'This check is not in this Help edition. A topic link is unavailable.' :
      current().extra ? 'Copy a link to the bundled check explanation.' : '';
    controls.back.disabled = state.index === 0; controls.forward.disabled = state.index === state.history.length - 1;
    for (const button of controls.contents.querySelectorAll('button')) {
      if (button.dataset.topic === (parent ?? id)) button.setAttribute('aria-current', 'page');
      else button.removeAttribute('aria-current');
    }
    renderSearch();
    const entry = current();
    const scroll = entry.scroll;
    requestAnimationFrame(() => {
      if (current() !== entry || controls.article.hidden) return;
      restoreScroll(scroll);
      if (focus) controls.main.focus({ preventScroll: true });
    });
    remember();
  }
  function open(destination, extra) {
    if (!topics.some(topic => topic.id === destination.split('#')[0]) && extra?.id !== destination.split('#')[0]) return;
    saveScroll();
    if (current().destination !== destination || JSON.stringify(current().explanation) !== JSON.stringify(extra?.explanation)) {
      state.history = state.history.slice(0, state.index + 1);
      state.history.push({ destination, scroll: 0, extra, explanation: extra?.explanation });
      state.history = state.history.slice(-100);
      state.index = state.history.length - 1;
    } else if (destination.includes('#')) current().scroll = 0;
    setQuery('');
    render(true);
  }

  function setQuery(value) {
    state.query = value.slice(0, 300);
    controls.search.value = state.query;
  }

  function renderSearch() {
    const query = state.query.trim();
    controls.clear.hidden = state.query === '';
    controls.article.hidden = !!query;
    controls.results.hidden = !query;
    controls.edition.hidden = !!query || !controls.edition.textContent;
    controls.breadcrumb.hidden = !!query || !controls.breadcrumb.firstChild;
    controls['result-list'].replaceChildren();
    if (!query) {
      controls['result-count'].textContent = '';
      return;
    }
    const words = query.toLowerCase().split(/\s+/);
    // Page order breaks ties. Classify before applying the display limit.
    const byRank = [[], [], []];
    for (const entry of searchable) {
      const { title, keywords, body } = entry;
      if (words.every(word => title.includes(word))) byRank[0].push(entry);
      else if (words.every(word => title.includes(word) || keywords.includes(word))) byRank[1].push(entry);
      else if (words.every(word => title.includes(word) || keywords.includes(word) || body.includes(word))) byRank[2].push(entry);
    }
    const matches = byRank.flat(), shown = matches.slice(0, 50);
    controls['result-count'].textContent = matches.length > shown.length ? `Showing ${shown.length} of ${matches.length} matching topics`
      : matches.length ? `${matches.length} matching ${matches.length === 1 ? 'topic' : 'topics'}`
      : `No Help results for “${query}”.`;
    for (const { topic } of shown) {
      const visible = topic.text.toLowerCase();
      const match = words.map(word => visible.indexOf(word)).find(position => position !== -1) ?? 0;
      const start = Math.max(0, match - 30), end = start + 160;
      const excerpt = `${start ? '…' : ''}${topic.text.slice(start, end).trim()}${end < topic.text.length ? '…' : ''}`;
      const item = node('li'), button = node('button', topic.title);
      button.type = 'button';
      button.addEventListener('click', () => open(topic.id));
      if (excerpt) button.append(node('small', excerpt));
      item.append(button);
      controls['result-list'].append(item);
    }
  }

  function search(value) {
    saveScroll();
    setQuery(value);
    renderSearch();
    if (controls.article.hidden) controls.main.scrollTop = 0;
    else restoreScroll();
    remember();
  }
  setQuery(state.query);
  controls.search.addEventListener('input', () => search(controls.search.value));
  controls.clear.addEventListener('click', () => { search(''); controls.search.focus(); });
  controls.permalink.addEventListener('click', () => api.postMessage({ type: 'permalink', destination: current().destination }));
  for (const [id, step] of [['back', -1], ['forward', 1]]) controls[id].addEventListener('click', () => {
    saveScroll();
    setQuery('');
    state.index = Math.min(state.history.length - 1, Math.max(0, state.index + step));
    render(true);
  });
  controls.main.addEventListener('scroll', () => { saveScroll(); remember(); }, { passive: true });
  document.addEventListener('click', event => {
    const link = event.target.closest('[data-link]');
    if (link) {
      event.preventDefault(); const target = link.dataset.link;
      if (target.startsWith('help:')) open(target.slice(5));
      else if (target.startsWith('check:')) open(target);
      else if (target.startsWith('tool:') || target.startsWith('option:')) open(target);
      else if (target.startsWith('#')) open(`${current().destination.split('#')[0]}${target}`, current().extra);
      else api.postMessage({ type: 'link', target });
    }
    const copy = event.target.closest('[data-copy]');
    if (copy) api.postMessage({ type: 'copy', text: copy.parentElement.querySelector('code').textContent });
    const image = event.target.closest('.image');
    if (image) { const original = image.querySelector('img'), enlarged = controls.enlarged.querySelector('img'); enlarged.src = original.src; enlarged.alt = original.alt; controls.enlarged.showModal(); }
  });
  controls['close-image'].addEventListener('click', () => controls.enlarged.close());
  window.addEventListener('message', event => {
    const message = event.data;
    if (message.type === 'bundle') {
      topics = message.topics; controls.contents.replaceChildren();
      searchable = topics.map(topic => ({ topic, title: topic.title.toLowerCase(), keywords: topic.keywords.join(' ').toLowerCase(), body: topic.body }));
      for (const [index, entry] of state.history.entries()) {
        const details = message.historyDetails?.[index];
        if (details && details.destination !== entry.destination) {
          entry.destination = details.destination;
          entry.scroll = 0;
        }
        entry.extra = details?.extra;
      }
      for (const topic of topics.filter(topic => !topic.id.includes(':'))) { const button = node('button', topic.title); button.type = 'button'; button.dataset.topic = topic.id; button.addEventListener('click', () => { if (narrow.matches) controls.topics.open = false; open(topic.id); }); controls.contents.append(button); }
      render();
    } else if (message.type === 'open') open(message.destination, message.extra);
    else if (message.type === 'notice') controls.notice.textContent = message.text;
  });
  api.postMessage({ type: 'ready', history: state.history.map(({ destination, explanation }) => ({ destination, explanation })) });
})();
