import { test } from 'node:test';
import assert from 'node:assert/strict';
import { filterProjects, restartName } from '../src/projects.ts';

const projects = [
  { name: 'Claims-analysis', brief: 'Research the insurance market' },
  { name: 'Board-proposal', brief: 'A plan for the claims team' },
  { name: 'Research-notes', brief: '' },
];
test('project search matches words across name and brief without changing order or identity', () => {
  assert.deepEqual(filterProjects(projects, ' CLAIMS  research '), [projects[0]]);
  const matches = filterProjects(projects, 'claims');
  assert.deepEqual(matches, [projects[0], projects[1]]);
  assert.equal(matches[1], projects[1]);
});
test('empty search shows all projects; missing words and an empty workspace show no matches', () => {
  assert.deepEqual(filterProjects(projects, '  '), projects);
  assert.deepEqual(filterProjects(projects, 'claims solar'), []);
  assert.deepEqual(filterProjects([], 'claims'), []);
});
test('fresh project names avoid collisions on case-insensitive filesystems and stay valid at the name limit', () => {
  assert.equal(restartName('case', ['case']), 'case-restart');
  assert.equal(restartName('case', ['case-restart', 'CASE-RESTART-2']), 'case-restart-3');
  const long = restartName('a'.repeat(64), []);
  assert.equal(long.length, 64);
  assert.match(long, /^[A-Za-z0-9_-]+$/);
  assert.notEqual(restartName('a'.repeat(64), [long]), long);
});

test('a suggested name comes from the brief, is a valid project name, and falls back to the outcome', async () => {
  const { suggestName } = await import('../src/projects.ts');
  const valid = /^[A-Za-z0-9_-]{1,64}$/;
  assert.equal(suggestName('writing', 'Write a newsletter about what our interviews changed'), 'newsletter-interviews-changed');
  assert.equal(suggestName('case_study', ''), 'case-study');
  assert.equal(suggestName('brainstorm', '!!! ??? '), 'brainstorm');
  const long = suggestName('writing', 'supercalifragilistic '.repeat(10));
  assert.match(long, valid);
  assert.ok(long.length <= 48);
  assert.match(suggestName('writing', 'Résumé für Ärzte, café'), valid);
  assert.equal(suggestName('research_brief', 'What did OpenRouter change about free model rate limits?'), 'openrouter-change-free-model');
});

test('last activity reads as minutes, today, yesterday or a date, and nothing when unknown', async () => {
  const { lastActivity } = await import('../src/projects.ts');
  const now = new Date(2026, 10, 10, 18, 54).getTime();
  assert.equal(lastActivity(null, now), '');
  assert.equal(lastActivity(now / 1000 - 20, now), 'Just now');
  assert.equal(lastActivity(now / 1000 - 12 * 60, now), '12 min ago');
  assert.match(lastActivity(new Date(2026, 10, 10, 9, 5).getTime() / 1000, now), /^Today 9:05/);
  assert.equal(lastActivity(new Date(2026, 10, 9, 22, 0).getTime() / 1000, now), 'Yesterday');
  assert.equal(lastActivity(new Date(2026, 10, 3, 12, 0).getTime() / 1000, now), 'Nov 3');
  assert.equal(lastActivity(new Date(2025, 0, 3, 12, 0).getTime() / 1000, now), 'Jan 3, 2025');
});

test('a repeated brief gets its own name instead of colliding with an existing project', async () => {
  const { uniqueName } = await import('../src/projects.ts');
  assert.equal(uniqueName('harbor lane', []), 'harbor lane');
  assert.equal(uniqueName('harbor lane', ['harbor lane']), 'harbor lane 2');
  assert.equal(uniqueName('harbor lane', ['harbor lane', 'harbor lane 2']), 'harbor lane 3');
  assert.equal(uniqueName('harbor lane', ['HARBOR LANE']), 'harbor lane 2', 'folders are case-insensitive on a Mac');
  // The app's project names have no spaces, so the caller can choose the separator.
  assert.equal(uniqueName('harbor-lane', ['harbor-lane', 'harbor-lane-2'], '-'), 'harbor-lane-3');
  const long = 'a'.repeat(48);
  const next = uniqueName(long, [long]);
  assert.ok(next.length <= 48);
  assert.notEqual(next, long);
  assert.ok(next.endsWith(' 2'));
  const tenth = uniqueName(long, [long, ...Array.from({ length: 8 }, (_, i) => `${'a'.repeat(46)} ${i + 2}`)]);
  assert.ok(tenth.length <= 48 && tenth.endsWith(' 10'), tenth);
});

test('a brief of only emoji, stop words or symbols still gives a valid non-empty name', async () => {
  const { suggestName } = await import('../src/projects.ts');
  const valid = /^[A-Za-z0-9_-]{1,64}$/;
  for (const brief of ['🍞 the a of', '🍞🍞🍞', '--- ---', '   ']) {
    const name = suggestName('writing', brief);
    assert.match(name, valid, brief);
  }
  assert.equal(suggestName('writing', '🍞 the a of'), 'writing');
});
