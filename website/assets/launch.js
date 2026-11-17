// Only same-origin release metadata is loaded. No accounts, cookies, analytics, or external scripts.
const download = document.querySelector('#release-download');
const source = document.querySelector('#source-download');
const notices = document.querySelector('#notices-download');
const setText = (selector, text) => { const node = document.querySelector(selector); if (node) node.textContent = text; };
const show = (selector) => { const node = document.querySelector(selector); if (node) node.hidden = false; };
if (download || source || notices) {
  fetch('/release.json', { cache: 'no-store' }).then((r) => {
    if (!r.ok) throw new Error('Release status unavailable');
    return r.json();
  }).then((release) => {
    if (!release || release.available !== true || !/^\d+\.\d+\.\d+$/.test(release.version)) return;
    const base = `https://download.sproutcrew.app/releases/${release.version}/SproutCrew_${release.version}`;
    const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
    const material = (item, suffix) => item && item.url === `${base}${suffix}` && hash(item.sha256);
    if (release.url !== `${base}_aarch64.dmg` || !hash(release.sha256) ||
        !material(release.materials?.source, '_mpl-sources.tar.gz') ||
        !material(release.materials?.notices, '_notices.zip')) return;
    for (const [node, url] of [[download, release.url], [source, release.materials.source.url], [notices, release.materials.notices.url]]) {
      if (node) { node.href = url; node.hidden = false; }
    }
    setText('#release-title', `SproutCrew ${release.version} · Free public beta`);
    setText('#release-status', 'Signed and notarized for Mac. Review the setup guide and beta limits before your first project.');
    setText('#sha256', release.sha256);
    setText('#source-sha256', release.materials.source.sha256);
    setText('#notices-sha256', release.materials.notices.sha256);
    show('#release-checksum');
    show('#materials-checksums');
  }).catch(() => {
    setText('#release-status', 'Release status could not be loaded. Please reload or check back later.');
  });
}
