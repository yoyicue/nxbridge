// Exercise the shipped script's state rendering without a browser or device.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.className = ''; this.textContent = ''; this.innerHTML = ''; this.disabled = false; this.style = {}; }
  append(...items) { this.children.push(...items); }
  appendChild(item) { this.children.push(item); }
  replaceChildren(...items) { this.children = items; }
  setAttribute() {}
  addEventListener(event, fn) { this[event] = fn; }
  showModal() { this.open = true; }
  close() { this.open = false; }
}
const nodes = Object.fromEntries(['endpoints', 'updated', 'refresh', 'game-list', 'add-game', 'catalog-dialog', 'catalog-form', 'form-error', 'cancel-edit', 'save-edit', 'dialog-title', 'dialog-help', 'game-name', 'game-id', 'build-id', 'game-version', 'game-directory', 'field-game-name', 'field-game-id', 'field-build-id', 'field-version', 'field-directory'].map(id => [id, new Element(id)]));
const document = {
  getElementById: id => nodes[id],
  createElement: tag => new Element(tag),
  createTextNode: text => ({textContent: text}),
};
let fail = false;
let posted;
const fetch = async (path, options = {}) => {
  if (fail) throw new Error('offline');
  if (options.method === 'POST') { posted = options; return {ok:true}; }
  if (path === '/api/v1/games') return {ok: true, json: async () => ({revision:3,write_enabled:true,games: [
    {id:'a', name:'<script>game</script>', cover_version:'abc', builds:[{id:'1',version:'1.9',status:'experimental',evidence:null,capabilities:{video:'experimental',input:'unknown'}}]},
    {id:'b', name:'Other game', builds:[]},
  ]})};
  return {ok: true, json: async () => ({sampled_at_unix_ms: 1_000_000, endpoints: [
    {label: '<img src=x onerror=alert(1)>', status:'ready'},
    {label: 'Second', status:'unreachable'},
  ]})};
};
const html = fs.readFileSync('web/index.html', 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/);
assert(script, 'page script missing');
const context = vm.createContext({document, fetch, AbortController, setTimeout, clearTimeout, setInterval: () => 0, Date});
vm.runInContext(script[1], context);

(async () => {
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(nodes.endpoints.children.length, 2);
  const first = nodes.endpoints.children[0];
  assert.equal(first.children[1].children[0].textContent, '<img src=x onerror=alert(1)>');
  assert.equal(first.children[1].children[0].innerHTML, '');
  assert.equal(first.children[2].className, 'pill ready');
  assert.equal(nodes.endpoints.children[1].children[2].className, 'pill unreachable');
  assert.equal(nodes['game-list'].children.length, 2);
  const title = nodes['game-list'].children[0].children[0].children[1].children[0];
  assert.equal(title.textContent, '<script>game</script>');
  assert.equal(title.innerHTML, '');
  const card = nodes['game-list'].children[0];
  assert.equal(card.children[0].children[0].children[0].src, '/api/v1/covers/a');
  const actions = card.children[1];
  assert.equal(actions.children[2].textContent, '更换封面');
  const image = {size: 100, type:'image/png'};
  actions.children[3].files = [image];
  await actions.children[3].change();
  assert.equal(posted.headers['X-NXBridge-Cover-Version'], 'abc');
  assert.equal(posted.headers['Content-Type'], 'image/png');
  assert.equal(posted.body, image);
  const build = nodes['game-list'].children[0].children.find(child => child.className === 'build');
  assert.equal(build.children[0].children[1].textContent, '登记：实验中');
  assert.equal(build.children[1].children[0].textContent, '尚未绑定构建目录');
  nodes['add-game'].click();
  assert.equal(nodes['catalog-dialog'].open, true);
  nodes['game-name'].value = 'Another game'; nodes['game-id'].value = 'game-c';
  nodes['build-id'].value = 'build-c'; nodes['game-version'].value = '1.0'; nodes['game-directory'].value = '/tmp/game';
  await nodes['catalog-form'].submit({preventDefault(){}});
  assert.equal(posted.headers['X-NXBridge-Request'], '1');
  assert.match(posted.body, /action = "add_build"/);
  assert.match(posted.body, /revision = 3/);
  assert.match(posted.body, /directory = "\/tmp\/game"/);
  assert.equal(nodes['catalog-dialog'].open, false);
  assert.match(nodes.updated.textContent, /^更新于 /);
  fail = true;
  await nodes.refresh.click();
  assert.equal(nodes.updated.textContent, '检查失败，请重试');
  assert.equal(nodes.endpoints.children[0].textContent, '无法获取主机状态。');
  assert.equal(nodes['game-list'].children[0].textContent, '无法读取游戏库。');
  console.log('PASS: dashboard status rendering and failure state');
})().catch(error => { console.error(error); process.exitCode = 1; });
