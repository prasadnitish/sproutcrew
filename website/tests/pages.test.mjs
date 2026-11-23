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

test('release timeline controls reach each version and its complete notes', () => {
  const page = read('changelog.html');
  const releases = [...page.matchAll(/<article class="release" id="([^"]+)"/g)].map(match => match[1]);
  assert.ok(releases.length >= 2);
  for (const id of releases) {
    assert.ok(page.includes(`href="#${id}" data-milestone=`));
    assert.ok(page.includes(`href="#notes-${id}"`));
    assert.ok(page.includes(`<details id="notes-${id}"><summary>`));
  }
});

test('the download and notices pages show the current release without JavaScript', () => {
  const release = JSON.parse(read('release.json'));
  const download = read('download.html');
  const notices = read('software-notices.html');
  if (release.available !== true) {
    assert.ok(!download.includes('download.sproutcrew.app'), 'no installer link while the release is unavailable');
    assert.ok(!notices.includes('download.sproutcrew.app'), 'no materials links while the release is unavailable');
    return;
  }
  assert.ok(download.includes(`<a id="release-download" class="button" href="${release.url}">`), 'download button links to the installer');
  assert.ok(download.includes(`<code id="sha256">${release.sha256}</code>`), 'download page shows the installer checksum');
  assert.ok(download.includes(`SproutCrew ${release.version} · Free public beta`), 'download page names the version');
  assert.ok(!/id="release-checksum" hidden/.test(download), 'checksum is visible');
  assert.ok(notices.includes(`<a id="source-download" href="${release.materials.source.url}">`));
  assert.ok(notices.includes(`<a id="notices-download" href="${release.materials.notices.url}">`));
  assert.ok(notices.includes(`<code id="source-sha256">${release.materials.source.sha256}</code>`));
  assert.ok(notices.includes(`<code id="notices-sha256">${release.materials.notices.sha256}</code>`));
  assert.ok(!/id="materials-checksums" hidden/.test(notices), 'materials checksums are visible');
  for (const html of [download, notices]) assert.ok(!html.includes('after the final release checks'), 'no pre-release wording');
});

test('the help search data parses, and every entry in it is an answer on the help page', () => {
  const entries = new Function(`${read('assets/help-data.js')}\n;return helpEntries;`)();
  assert.ok(Array.isArray(entries) && entries.length > 0);
  const page = read('help.html');
  for (const entry of entries) {
    assert.ok(entry.id && entry.q && entry.a, `entry ${entry.id} has an id, a question and an answer`);
    assert.ok(page.includes(`id="${entry.id}"`), `help.html has no answer with id ${entry.id}`);
  }
});
