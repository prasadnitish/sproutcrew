import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const script = readFileSync(new URL('../assets/launch.js', import.meta.url), 'utf8');
async function load(release, fail = false, materialsOnly = false) {
  const ids = ['release-title', 'release-status', 'sha256', 'release-checksum', 'source-download', 'notices-download', 'source-sha256', 'notices-sha256', 'materials-checksums'];
  if (!materialsOnly) ids.push('release-download');
  const nodes = new Map(ids.map(id => [id, { hidden: true, href: '', textContent: '' }]));
  const requests = [];
  vm.runInNewContext(script, {
    document: { querySelector: selector => nodes.get(selector.slice(1)) },
    fetch: async (url, options) => {
      requests.push([url, options.cache]);
      if (fail) throw new Error('offline');
      return { ok: true, json: async () => release };
    },
  });
  await new Promise(resolve => setImmediate(resolve));
  return { nodes, requests };
}
const candidate = { available: true, version: '2.0.1', url: 'https://download.sproutcrew.app/releases/2.0.1/SproutCrew_2.0.1_aarch64.dmg', sha256: 'a'.repeat(64), materials: {
  source: { url: 'https://download.sproutcrew.app/releases/2.0.1/SproutCrew_2.0.1_mpl-sources.tar.gz', sha256: 'b'.repeat(64) },
  notices: { url: 'https://download.sproutcrew.app/releases/2.0.1/SproutCrew_2.0.1_notices.zip', sha256: 'c'.repeat(64) },
} };

test('an approved release exposes only its expected immutable URL and checksum', async () => {
  const { nodes, requests } = await load(candidate);
  assert.deepEqual(requests, [['/release.json', 'no-store']]);
  assert.equal(nodes.get('release-download').href, candidate.url);
  assert.equal(nodes.get('release-download').hidden, false);
  assert.equal(nodes.get('sha256').textContent, candidate.sha256);
  assert.equal(nodes.get('source-download').href, candidate.materials.source.url);
  assert.equal(nodes.get('notices-download').hidden, false);
});

test('drafts and invalid metadata never expose a download', async () => {
  for (const release of [
    { ...candidate, available: false },
    { ...candidate, available: 'false' },
    { ...candidate, url: 'https://example.com/untrusted.dmg' },
    { ...candidate, url: 'https://download.sproutcrew.app/releases/2.0.1/SproutCrew_2.0.1_universal.dmg' },
    { ...candidate, version: '../2.0.1' },
    { ...candidate, sha256: null },
    { ...candidate, sha256: 'x'.repeat(64) },
    { ...candidate, materials: undefined },
    { ...candidate, materials: { ...candidate.materials, source: { ...candidate.materials.source, url: 'https://example.com/source.tar.gz' } } },
    { ...candidate, materials: { ...candidate.materials, notices: { ...candidate.materials.notices, sha256: null } } },
    {},
  ]) {
    const { nodes } = await load(release);
    assert.equal(nodes.get('release-download').hidden, true);
    assert.equal(nodes.get('release-download').href, '');
    assert.equal(nodes.get('source-download').hidden, true);
    assert.equal(nodes.get('notices-download').hidden, true);
  }
});

test('the software notices page exposes the same versioned materials without an installer button', async () => {
  const { nodes, requests } = await load(candidate, false, true);
  assert.deepEqual(requests, [['/release.json', 'no-store']]);
  assert.equal(nodes.get('source-download').hidden, false);
  assert.equal(nodes.get('source-sha256').textContent, candidate.materials.source.sha256);
  assert.equal(nodes.get('notices-sha256').textContent, candidate.materials.notices.sha256);
});

test('an unavailable release manifest gives a usable error', async () => {
  const { nodes } = await load(null, true);
  assert.equal(nodes.get('release-download').hidden, true);
  assert.match(nodes.get('release-status').textContent, /could not be loaded/);
});
