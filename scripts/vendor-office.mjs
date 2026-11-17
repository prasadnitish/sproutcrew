// Run after npm ci --ignore-scripts in crates/bff-core/export-js. No CDN/runtime downloads.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const base = path.join(root, 'crates/bff-core/export-js');
const lock = JSON.parse(fs.readFileSync(path.join(base, 'package-lock.json')));
const vendor = path.join(base, 'vendor');
fs.mkdirSync(vendor, { recursive: true });
// Use the locked JSZip, rather than the older JSZip embedded in PptxGenJS's prebuilt bundle.
const inputs = ['node_modules/jszip/dist/jszip.min.js', 'node_modules/pptxgenjs/dist/pptxgen.min.js'];
const source = Buffer.concat(inputs.flatMap(file => [fs.readFileSync(path.join(base, file)), Buffer.from('\n')]));
fs.writeFileSync(path.join(vendor, 'pptxgen.bundle.js'), source);
const notices = ['SproutCrew Office export JavaScript notices',
  'Unmodified upstream JSZip and PptxGenJS browser files are concatenated. MIT is selected for dual MIT/GPL JSZip.',
  'The dependency inventory also includes Node-only dependencies, which are not exposed in the embedded runtime.'];
const packages = [];
for (const [entry, info] of Object.entries(lock.packages)) {
  if (!entry.startsWith('node_modules/') || info.dev) continue;
  const name = entry.split('node_modules/').at(-1);
  // npm's placeholder for Node's built-in HTTPS module is not part of the browser bundle.
  if (name === 'https') continue;
  const folder = path.join(base, entry);
  const files = fs.readdirSync(folder).filter(n => /^(license|copying|copyright|notice)/i.test(n));
  if (name === 'isarray') files.push('README.md'); // upstream carries the complete MIT grant here
  if (!files.length) throw new Error(`Missing license for ${name}`);
  for (const file of files) {
    const filename = path.join(folder, file);
    if (fs.statSync(filename).isFile()) notices.push(`\n${'='.repeat(72)}\n${name} ${info.version} / ${file}\n${fs.readFileSync(filename, 'utf8')}`);
  }
  packages.push({ name, version: info.version, license: info.license, source: info.resolved, integrity: info.integrity });
}
fs.writeFileSync(path.join(vendor, 'THIRD-PARTY-NOTICES.txt'), notices.join('\n') + '\n');
fs.writeFileSync(path.join(vendor, 'manifest.json'), JSON.stringify({
  bundle: 'pptxgen.bundle.js', sha256: crypto.createHash('sha256').update(source).digest('hex'),
  inputs: inputs.map(file => ({file, sha256: crypto.createHash('sha256').update(fs.readFileSync(path.join(base,file))).digest('hex')})),
  package: 'pptxgenjs', version: '4.0.1', packages,
}, null, 2) + '\n');
console.log(`Vendored ${source.length} bytes and ${packages.length} package notices`);
