/* 一次性校验：编码 / 标签闭合 / id 引用 / JS 语法 / 假 DOM 里跑 render + 事件委托 + 状态机 */
const fs = require('fs'), vm = require('vm');
const src = fs.readFileSync('prototype.html', 'utf8');
const js = src.match(/<script>([\s\S]*?)<\/script>/)[1];
const keep = (s, re) => s.replace(re, m => '\n'.repeat((m.match(/\n/g) || []).length));
const markup = keep(keep(src, /<style>[\s\S]*?<\/style>/g), /<script>[\s\S]*?<\/script>/g);
const fail = [];

if (src.includes('\uFFFD')) fail.push('encoding: U+FFFD present');
if (!/<html[^>]*data-theme="ledger"/.test(src)) fail.push('default theme is not ledger');
if (!/prefers-reduced-motion/.test(src)) fail.push('missing prefers-reduced-motion');
if (!/data-act=exit/.test(src)) fail.push('missing 真正退出 entry');

/* 1 · 标签闭合 */
const voids = new Set(['meta', 'link', 'br', 'hr', 'img', 'input', 'source', 'use', 'path', 'rect', 'circle', 'ellipse', 'polygon', 'line', 'polyline']);
const lineOf = i => markup.slice(0, i).split('\n').length;
const stack = [];
for (let m, re = /<(\/?)([a-zA-Z][\w-]*)\b([^>]*)>/g; (m = re.exec(markup));) {
  const closing = m[1] === '/', name = m[2].toLowerCase();
  if (voids.has(name) || /\/\s*$/.test(m[3])) continue;
  if (!closing) stack.push({ t: name, l: lineOf(m.index) });
  else if (!stack.length || stack[stack.length - 1].t !== name) {
    fail.push('</' + name + '> line ' + lineOf(m.index) + ' stack: ' + stack.slice(-5).map(s => s.t + '@' + s.l).join('>'));
    const i = stack.map(s => s.t).lastIndexOf(name);
    if (i < 0) break; stack.length = i;
  } else stack.pop();
}
if (stack.length) fail.push('unclosed: ' + stack.map(s => s.t + '@' + s.l).join('>'));

/* 2 · JS 引用的 id 必须真实存在 */
const ids = new Set([...markup.matchAll(/\sid="([^"]+)"/g)].map(m => m[1]));
for (const m of js.matchAll(/\$\('([\w-]+)'\)/g)) if (!ids.has(m[1])) fail.push('missing id #' + m[1]);

/* 3 · 假 DOM */
const L = {}, store = {}, qOne = {}, cls = new Map(), ds = new Map(), at = new Map();
const mk = id => {
  const el = {
    id, _html: '', textContent: '', value: '', scrollTop: 0, scrollHeight: 999, disabled: false,
    style: {}, parentElement: null, activeElement: null,
    dataset: ds.get(id) || (() => { const d = {}; ds.set(id, d); return d; })(),
    classList: {
      add: c => { const s = cls.get(id) || new Set(); s.add(c); cls.set(id, s); },
      remove: c => { (cls.get(id) || new Set()).delete(c); },
      toggle: c => { const s = cls.get(id) || new Set(); s.has(c) ? s.delete(c) : s.add(c); cls.set(id, s); },
      contains: c => (cls.get(id) || new Set()).has(c)
    },
    addEventListener(t, f) { (L[id + ':' + t] = L[id + ':' + t] || []).push(f); },
    setAttribute(k, v) { const o = at.get(id) || {}; o[k] = v; at.set(id, o); },
    getAttribute(k) { return (at.get(id) || {})[k]; },
    focus() {}, blur() {},
    querySelector(sel) { return qOne[id + '|' + sel] !== undefined ? qOne[id + '|' + sel] : mk(id + '|' + sel); },
    querySelectorAll() { return []; },
    get innerHTML() { return this._html; }, set innerHTML(v) { this._html = v; }
  };
  return el;
};
const get = id => (store[id] = store[id] || mk(id));
const doc = {
  documentElement: get('html'), body: get('body'), activeElement: null,
  addEventListener(t, f) { (L['doc:' + t] = L['doc:' + t] || []).push(f); },
  querySelector: sel => (qOne[sel] !== undefined ? qOne[sel] : null),
  querySelectorAll: () => [], getElementById: get, createElement: t => mk('<' + t + '>')
};
const timers = [];
const ctx = vm.createContext({
  document: doc, console, setInterval: f => (timers.push(f), 1), setTimeout: f => (timers.push(f), 1),
  clearTimeout() {}, clearInterval() {}, requestAnimationFrame: f => (timers.push(f), 1), Date, Math, JSON,
  navigator: { clipboard: { writeText: () => Promise.resolve() } }
});
ctx.window = ctx;
const node = d => ({ closest: () => null, dataset: d || {}, textContent: 'x', value: '', focus() {}, classList: mk('node').classList });
const fire = (key, pairs, extra) => {
  const ev = Object.assign({
    target: { closest: sel => (pairs[sel] ? Object.assign(node(pairs[sel].dataset), pairs[sel]) : null) },
    preventDefault() {}
  }, extra || {});
  (L[key] || []).forEach(f => f(ev));
};
const act = (a, extra) => fire('doc:click', { '[data-act]': { dataset: Object.assign({ act: a }, extra || {}) } });
try { vm.runInContext(js, ctx, { filename: 'inline.js' }); } catch (e) { fail.push('script threw: ' + e.message); }

/* 3a · 六个会话：每个会话的证据都渲染得出来，且没有 undefined/NaN 漏字 */
try {
  for (const k of Object.keys(ctx.S)) {
    ctx.cur = k; ctx.render();
    const s = ctx.S[k], out = get('out').innerHTML, logs = get('logBox').innerHTML,
      runs = get('runList').innerHTML, yaml = get('yamlBox').innerHTML,
      dep = get('depList').innerHTML, app = get('appLog').innerHTML;
    if (out.includes('caret') !== !!ctx.ALIVE[s.s]) fail.push(k + ': caret/alive mismatch');
    if (!logs.includes('lrow') || !runs.includes('tl"') || !dep.includes('<li') || !app.includes('<li'))
      fail.push(k + ': pane / inspector content missing');
    if (/undefined|NaN|\[object/.test(out + logs + runs + yaml + dep + app)) fail.push(k + ': bad interpolation');
    if (s.mode === 'off' && s.rec[0] !== 'off') fail.push(k + ': mode/rec disagree');
    console.log(k.padEnd(12), s.s.padEnd(8), 'rec=' + s.rec[0].padEnd(4), 'mode=' + s.mode.padEnd(9),
      'out=' + String(out.length).padStart(4), 'logs=' + String(logs.length).padStart(4),
      'runs=' + String(runs.length).padStart(4), 'yaml=' + String(yaml.length).padStart(4));
  }

  /* 3b · 事件委托：点左侧会话 → 三个问题跟着换 */
  ctx.cur = 'comfyui'; ctx.render();
  fire('doc:click', { '.item': { dataset: { key: 'testapi' } } });
  if (ctx.cur !== 'testapi') fail.push('click .item did not select session');
  if (!get('qImpact').innerHTML.includes('没什么不能关的')) fail.push('三个问题没有跟着会话换');

  /* 3c · 日志策略五档：每档都要有自己的说法，off 不许声称有文件 */
  ctx.cur = 'tmp'; ctx.render();
  const origNote = ctx.S.tmp.note;
  ['manual', 'on_error', 'auto', 'always', 'off'].forEach(v => {
    fire('doc:click', { '[data-mode]': { dataset: { mode: v } } });
    const s = ctx.S.tmp;
    if (s.mode !== v) fail.push('mode pill ' + v + ' not applied');
    if (!s.rec[3] || s.rec[3].includes('undefined')) fail.push('mode ' + v + ' rec meta broken: ' + s.rec[3]);
    if (!s.note) fail.push('mode ' + v + ' note missing');
    if ((v === 'off' || v === 'manual') && s.rec[2].startsWith('logs/'))
      fail.push('mode ' + v + ' unexpectedly claims a run file');
    console.log('  mode=' + v.padEnd(9), 'chip=' + s.rec[1].padEnd(18), 'path=' + s.rec[2].slice(0, 40));
  });
  if (ctx.S.tmp.note !== origNote) fail.push('回到原策略后没有还原原本的说明');

  /* 3d · 主题：只换皮 */
  act('theme', { t: 'ink' });
  if (doc.documentElement.dataset.theme !== 'ink') fail.push('theme → ink failed');
  act('theme', { t: 'ledger' });
  if (doc.documentElement.dataset.theme !== 'ledger') fail.push('theme → ledger failed');

  /* 3e · 三个动词：启动 / 优雅停止（再点一次 = 强制） / 强制结束 */
  ctx.cur = 'kobold'; ctx.S.kobold.s = 'stopped'; ctx.render();
  if (get('btnStart').disabled) fail.push('stopped 会话本该能启动');
  if (get('btnStop').disabled === false) fail.push('stopped 会话不该能停止');
  act('force'); if (ctx.S.kobold.s !== 'stopped') fail.push('force on stopped changed state');
  ctx.startFlow(); if (ctx.S.kobold.s !== 'running') fail.push('startFlow failed');
  ctx.stopFlow(); if (ctx.S.kobold.s !== 'stopping') fail.push('stopFlow did not enter stopping');
  if (!get('stopLabel').textContent.includes('优雅停止中')) fail.push('stop label not counting down');
  ctx.stopFlow(); if (ctx.S.kobold.s !== 'stopped') fail.push('second stop did not force');
  timers.forEach(f => { try { f(); } catch (e) { fail.push('timer: ' + e.message); } });

  /* 3f · 覆盖层：托盘 / 真正退出（默认不是杀掉）/ 检验单 / Esc */
  act('tray'); if (!get('trayPop').classList.contains('open')) fail.push('tray did not open');
  act('exit'); if (!get('exitScrim').classList.contains('open')) fail.push('exit dialog did not open');
  qOne['.opt.sel'] = { dataset: { opt: 'stopall' } };
  act('doexit');
  if (get('exitScrim').classList.contains('open')) fail.push('exit dialog did not close');
  act('insp'); if (!get('win').classList.contains('insp')) fail.push('inspector did not toggle');
  fire('doc:keydown', {}, { key: 'Escape' });
  if (get('trayPop').classList.contains('open') || get('exitScrim').classList.contains('open'))
    fail.push('Escape did not dismiss overlays');

  /* 3g · stdin 永不进日志（LOGGING §4）：回车只发进 ConPTY，日志里只留一条说明 */
  ctx.cur = 'scratch'; ctx.render();
  const inEl = get('stdinInput'), before = ctx.S.scratch.logs.length;
  inEl.value = 'Get-Process | Select-Object -First 3';
  (L['stdinInput:keydown'] || []).forEach(f => f.call(inEl, { key: 'Enter', target: inEl }));
  if (ctx.S.scratch.logs.length !== before + 1) fail.push('stdin Enter did not record exactly one line');
  if (!/永不写进日志/.test(ctx.S.scratch.logs[ctx.S.scratch.logs.length - 1][3]))
    fail.push('stdin line does not state the policy');
  if (!/→ PTY/.test(get('out').innerHTML)) fail.push('stdin echo missing from terminal');
} catch (e) { fail.push('runtime: ' + e.message + ' | ' + String(e.stack).split('\n')[1]); }

try { new Function(js); } catch (e) { fail.push('syntax: ' + e.message); }
console.log(fail.length ? '\nFAIL\n' + fail.join('\n') : '\nALL CHECKS PASSED');
process.exit(fail.length ? 1 : 0);

