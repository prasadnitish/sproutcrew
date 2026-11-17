import { test } from 'node:test';
import assert from 'node:assert/strict';
import { contextLine, filterModels, groups, makers, money, priceLine, type CatalogModel } from '../src/models.ts';

const m = (id: string, over: Partial<CatalogModel> = {}): CatalogModel => ({
  id, name: id, maker: id.split('/')[0], context_length: 128000, input_per_mtok: 1, output_per_mtok: 5, free: false, sees_images: false, ...over,
});
const list = [
  m('anthropic/claude-haiku-5.5', { name: 'Claude Haiku 5.5', input_per_mtok: 0.1, output_per_mtok: 0.5 }),
  m('openrouter/free', { name: 'Free Models Router', input_per_mtok: 0, output_per_mtok: 0, free: true }),
  m('openrouter/auto', { name: 'Auto Router', input_per_mtok: null, output_per_mtok: null }),
  m('mistralai/mistral-small-2603', { name: 'Mistral Small' }),
  m('meta-llama/llama-4-scout:free', { free: true, input_per_mtok: 0, output_per_mtok: 0 }),
];

test('an unknown price reads as unknown and never as zero', () => {
  assert.equal(money(null), 'unknown');
  assert.equal(money(-1), 'unknown');
  assert.equal(money(0), '$0');
  assert.equal(money(0.1), '$0.10');
  assert.equal(money(0.028), '$0.028');
  assert.equal(priceLine(list[2]), 'Price unknown');
  assert.equal(priceLine(m('x/y', { output_per_mtok: null })), '$1.00 in · unknown out per million tokens');
  assert.equal(priceLine(list[0]), '$0.10 in · $0.50 out per million tokens');
  assert.equal(priceLine(list[1]), 'Free');
});

test('context length reads in thousands or millions of tokens', () => {
  assert.equal(contextLine(1_048_576), '1M context');
  assert.equal(contextLine(128_000), '128K context');
  assert.equal(contextLine(null), '');
});

test('search matches every word in the name or the ID, and the maker filter narrows it', () => {
  assert.deepEqual(filterModels(list, 'haiku 5.5', '').map((x) => x.id), ['anthropic/claude-haiku-5.5']);
  assert.deepEqual(filterModels(list, 'ROUTER', '').map((x) => x.id), ['openrouter/free', 'openrouter/auto']);
  assert.deepEqual(filterModels(list, '', 'mistralai').map((x) => x.id), ['mistralai/mistral-small-2603']);
  assert.deepEqual(filterModels(list, 'haiku', 'mistralai'), []);
  assert.deepEqual(makers(list), ['anthropic', 'meta-llama', 'mistralai', 'openrouter']);
});

test('each model shows once, in Recommended, then Free, then All models', () => {
  const g = groups(list, ['anthropic/claude-haiku-5.5', 'not/in-the-list']);
  assert.deepEqual(g.map((x) => x.title), ['Recommended', 'Free', 'All models']);
  assert.deepEqual(g[0].models.map((x) => x.id), ['anthropic/claude-haiku-5.5']);
  assert.deepEqual(g[1].models.map((x) => x.id), ['openrouter/free', 'meta-llama/llama-4-scout:free']);
  assert.deepEqual(g[2].models.map((x) => x.id), ['openrouter/auto', 'mistralai/mistral-small-2603']);
  assert.deepEqual(groups([], ['a/b']), []);
});
