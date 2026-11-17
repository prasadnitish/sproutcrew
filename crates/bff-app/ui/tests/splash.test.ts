import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isSplashSkipKey, REDUCED_SPLASH_DURATION_MS, SPLASH_DURATION_MS } from '../src/brand/splash.ts';
test('splash is bounded; reduced motion removes the startup wait', () => {
  assert.ok(SPLASH_DURATION_MS <= 2600);
  assert.ok(REDUCED_SPLASH_DURATION_MS <= 500);
  assert.ok(REDUCED_SPLASH_DURATION_MS < SPLASH_DURATION_MS);
});
test('skip keys work without swallowing navigation or modifier keys', () => {
  for (const key of ['Escape', 'Enter', ' ']) assert.equal(isSplashSkipKey(key), true);
  for (const key of ['Tab', 'Shift', 'Control', 'Meta', 'ArrowRight', 'a']) assert.equal(isSplashSkipKey(key), false);
});
