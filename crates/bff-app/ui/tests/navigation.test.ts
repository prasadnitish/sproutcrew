import { test } from 'node:test';
import assert from 'node:assert/strict';
import { choiceIndex } from '../src/navigation.ts';
test('single-selection navigation wraps, supports endpoints, and leaves text-entry keys alone', () => {
  assert.equal(choiceIndex('ArrowRight', 2, 3), 0);
  assert.equal(choiceIndex('ArrowLeft', 0, 3), 2);
  assert.equal(choiceIndex('ArrowDown', 0, 3), 1);
  assert.equal(choiceIndex('ArrowUp', 2, 3), 1);
  assert.equal(choiceIndex('Home', 2, 3), 0);
  assert.equal(choiceIndex('End', 0, 3), 2);
  for (const key of ['Tab', 'Enter', ' ', 'a']) assert.equal(choiceIndex(key, 1, 3), -1);
  assert.equal(choiceIndex('ArrowRight', 0, 0), -1);
  assert.equal(choiceIndex('ArrowRight', -1, 3), -1);
});
