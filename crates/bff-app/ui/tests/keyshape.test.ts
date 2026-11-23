import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { findKeyShape, isKeyInBrief, keyInBriefText, withoutKey } from '../src/keyshape.ts';

const key = `sk-or-v1-${'0123456789abcdef'.repeat(4)}`;

test('an OpenRouter-shaped key in a brief is found, with punctuation around it ignored', () => {
  assert.deepEqual(findKeyShape(`Write about bread. Here is my key: ("${key}"), thanks`), { key, openrouter: true });
});
test('other providers keys are found but are not offered to OpenRouter', () => {
  const anthropic = `sk-ant-api03-${'a1'.repeat(20)}`;
  assert.deepEqual(findKeyShape(`use ${anthropic}`), { key: anthropic, openrouter: false });
});
test('ordinary prose, including the word secret, has no key in it', () => {
  assert.equal(findKeyShape('Our secret: slow-fermented dough. A token of thanks: every tenth loaf is free.'), null);
  assert.equal(findKeyShape('Write 700 words for product leaders about the roadmap.'), null);
  assert.equal(findKeyShape('id 123e4567-e89b-12d3-a456-426614174000 and a hash ' + 'ab'.repeat(16)), null);
});
test('taking the key out leaves the rest of the brief tidy', () => {
  assert.equal(withoutKey(`Write about bread. ${key} Keep it short.`, key), 'Write about bread. Keep it short.');
  assert.equal(withoutKey(key, key), '');
  assert.equal(withoutKey(`line one\n${key}\nline two`, key), 'line one\n\nline two');
});
test('the error code is recognised and removed from the sentence', () => {
  const e = 'key-in-brief: This looks like a key. Keys go in the Connect box, not the brief.';
  assert.ok(isKeyInBrief(e));
  assert.equal(keyInBriefText(e), 'This looks like a key. Keys go in the Connect box, not the brief.');
  assert.ok(!isKeyInBrief('A project called "x" already exists.'));
});
test('the lines shared with the Rust scanner are found, or not found, exactly as the backend flags them', () => {
  // OpenRouter-shaped test keys are stored as {OPENROUTER_KEY_64} and made here, as in the Rust test.
  const fake = (n: number) => `sk-or-v1-${'0123456789abcdef'.repeat(n / 16)}`;
  const raw = readFileSync(new URL('../../../bff-core/tests/fixtures/key_shapes.json', import.meta.url), 'utf8').replaceAll('{OPENROUTER_KEY_64}', fake(64)).replaceAll('{OPENROUTER_KEY_48}', fake(48));
  const lines: { text: string; found: string | null; as?: string }[] = JSON.parse(raw);
  assert.ok(lines.length > 10);
  for (const { text, found, as } of lines) assert.equal(findKeyShape(text, as === 'name')?.key ?? null, found, text);
});
test('a key after = or after Bearer is found and, for OpenRouter, offered to OpenRouter', () => {
  assert.deepEqual(findKeyShape(`OPENROUTER_API_KEY=${key}`), { key, openrouter: true });
  assert.equal(findKeyShape('Authorization: Bearer abcdefghij1234567890zz')?.openrouter, false);
});
