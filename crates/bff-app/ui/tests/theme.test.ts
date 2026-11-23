import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyTheme, loadTheme } from '../src/theme.ts';

test('theme preference survives reload and returns to the system without an explicit override', () => {
  const store = new Map<string, string>();
  const dataset: Record<string, string> = {};
  const oldDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const oldStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  try {
    Object.defineProperty(globalThis, 'document', { configurable: true, value: { documentElement: { dataset } } });
    Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => store.set(key, value), removeItem: (key: string) => store.delete(key) } });
    assert.equal(loadTheme(), 'system');
    applyTheme('dark');
    assert.equal(dataset.theme, 'dark');
    assert.equal(loadTheme(), 'dark');
    applyTheme('system');
    assert.equal(dataset.theme, undefined);
    assert.equal(store.has('sproutcrew-theme'), false);
    assert.equal(loadTheme(), 'system');
    store.set('sproutcrew-theme', 'corrupt');
    assert.equal(loadTheme(), 'system');
    Object.defineProperty(globalThis, 'localStorage', { configurable: true, get() { throw new Error('Storage blocked'); } });
    assert.equal(loadTheme(), 'system');
    assert.doesNotThrow(() => applyTheme('light'));
    assert.equal(dataset.theme, 'light');
  } finally {
    if (oldDocument) Object.defineProperty(globalThis, 'document', oldDocument); else Reflect.deleteProperty(globalThis, 'document');
    if (oldStorage) Object.defineProperty(globalThis, 'localStorage', oldStorage); else Reflect.deleteProperty(globalThis, 'localStorage');
  }
});
