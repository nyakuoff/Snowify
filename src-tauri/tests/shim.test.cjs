// Native-only titlebar wiring. Run with `node --test src-tauri/tests/shim.test.cjs`.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('titlebar drag, descendants, double-click, and non-drag controls', () => {
  const calls = [];
  const handlers = new Map();
  class Element {
    constructor(drag = false) { this.drag = drag; }
    closest(selector) { return selector === '.titlebar-drag' && this.drag ? this : null; }
    setAttribute() {}
    getAttribute() { return null; }
  }
  const mediaClass = () => {
    class Media extends Element {}
    Object.defineProperty(Media.prototype, 'src', { configurable: true, get() { return ''; }, set() {} });
    return Media;
  };
  const context = vm.createContext({
    Element, HTMLMediaElement: mediaClass(), HTMLImageElement: mediaClass(), HTMLSourceElement: mediaClass(),
    MutationObserver: class { observe() {} },
    URL,
    document: { addEventListener: (name, fn) => handlers.set(name, fn) },
    window: {
      Audio: class {},
      __SNOWIFY_BOOT__: { platform: 'linux', proxy: { port: 1, token: 'test' } },
      __TAURI_INTERNALS__: {
        invoke: (command, args) => { calls.push({ command, args }); return Promise.resolve(); },
        transformCallback: () => 0,
      },
    },
  });
  const root = path.join(__dirname, '..');
  vm.runInContext(`(() => {\n${fs.readFileSync(path.join(root, 'codec.js'), 'utf8')}\n${fs.readFileSync(path.join(root, 'shim.js'), 'utf8')}\n})();`, context);
  calls.length = 0;
  const press = (target, button = 0, detail = 1) => {
    let prevented = false;
    handlers.get('mousedown')({ target, button, detail, preventDefault() { prevented = true; } });
    return prevented;
  };
  assert.equal(press(new Element(true)), true);
  assert.equal(calls.at(-1).args.channel, 'window:startDragging');
  assert.equal(calls.at(-1).command, 'ipc_send');
  assert.equal(press(new Element(true), 0, 2), true);
  assert.equal(calls.at(-1).args.channel, 'window:maximize');
  for (const [target, button] of [[new Element(false), 0], [new Element(true), 2], [{}, 0]]) {
    assert.equal(press(target, button), false);
  }
  assert.equal(calls.length, 2, 'controls, non-elements and right-click must not move the window');
});
