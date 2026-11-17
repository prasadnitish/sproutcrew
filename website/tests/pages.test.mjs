import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';

const root = new URL('../', import.meta.url);
const read = name => readFileSync(new URL(name, root), 'utf8');
const pages = readdirSync(root).filter(name => name.endsWith('.html'));

test('the newest changelog entry is the version the download page offers', () => {
  const release = JSON.parse(read('release.json'));
  const first = read('changelog.html').match(/<article class="release" id="v([0-9-]+)"/);
  assert.ok(first, 'the changelog has a first release');
  assert.equal(first[1].replaceAll('-', '.'), release.version);
});

test('every changelog entry has a date a machine can read, newest first', () => {
  const dates = [...read('changelog.html').matchAll(/<time datetime="(\d{4}-\d{2}-\d{2})">/g)].map(m => m[1]);
  assert.ok(dates.length >= 2);
  assert.deepEqual(dates, [...dates].sort().reverse());
});

test('every local link on every page leads to a file that is published', () => {
  for (const page of pages) {
    for (const [, target] of read(page).matchAll(/(?:href|src)="(\/[^"#?]*)/g)) {
      const path = target === '/' ? 'index.html' : target.slice(1);
      assert.ok(existsSync(new URL(path, root)) || existsSync(new URL(`${path}.html`, root)), `${page} links to ${target}`);
    }
  }
});

test('the changelog, roadmap and process pages are in the sitemap and link to each other', () => {
  const sitemap = read('sitemap.xml');
  for (const slug of ['changelog', 'roadmap', 'how-its-built']) {
    assert.match(sitemap, new RegExp(`https://www.sproutcrew.app/${slug}<`));
    const html = read(`${slug}.html`);
    for (const other of ['changelog', 'roadmap', 'how-its-built']) assert.ok(html.includes(`href="/${other}"`), `${slug} links to ${other}`);
  }
});

test('no page loads a script or style from another site', () => {
  for (const page of pages) {
    assert.doesNotMatch(read(page), /<script[^>]+src="https?:\/\/|<link[^>]+rel="(?:stylesheet|preload)"[^>]+href="https?:\/\//, page);
  }
});
