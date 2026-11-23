import { test } from 'node:test';
import assert from 'node:assert/strict';
import { addedLine, sizeLabel, pathLabel } from '../src/addedFiles.ts';

test('sizes read in KB under a megabyte, never below 1 KB, and in MB above', () => {
  assert.equal(sizeLabel(0), '1 KB');
  assert.equal(sizeLabel(12 * 1024), '12 KB');
  assert.equal(sizeLabel(1024 * 1024), '1.0 MB');
  assert.equal(sizeLabel(2.5 * 1024 * 1024), '2.5 MB');
});

test('the count line says what was added and is empty when nothing was', () => {
  assert.equal(addedLine([]), '');
  assert.equal(addedLine([{ kind: 'file' }]), '1 file added');
  assert.equal(addedLine([{ kind: 'file' }, { kind: 'path' }]), '2 files added');
  assert.equal(addedLine([{ kind: 'url' }]), '1 web address added');
  assert.equal(addedLine([{ kind: 'file' }, { kind: 'url' }, { kind: 'url' }]), '1 file and 2 web addresses added');
  assert.equal(addedLine([{ kind: 'results' }]), '1 set of project results added');
  assert.equal(addedLine([{ kind: 'file' }, { kind: 'url' }, { kind: 'results' }]), '1 file, 1 web address and 1 set of project results added');
});

test('a folder is counted as a folder, not a file', () => {
  assert.equal(addedLine([{ kind: 'path', folder: true }]), '1 folder added');
  assert.equal(addedLine([{ kind: 'path', folder: true }, { kind: 'path', folder: true }]), '2 folders added');
  assert.equal(addedLine([{ kind: 'path' }, { kind: 'file' }, { kind: 'path', folder: true }]), '2 files and 1 folder added');
  assert.equal(addedLine([{ kind: 'path', folder: true }, { kind: 'url' }]), '1 folder and 1 web address added');
});

test('a path shows its name and size, a folder says folder, and an unread path shows as typed', () => {
  assert.equal(pathLabel('~/Documents/claims.pdf', { name: 'claims.pdf', size: 12288, is_dir: false }), 'claims.pdf · 12 KB');
  assert.equal(pathLabel('~/Documents/notes', { name: 'notes', size: null, is_dir: true }), 'notes · folder');
  assert.equal(pathLabel('~/gone.pdf', { name: 'gone.pdf', size: null, is_dir: false }), 'gone.pdf');
  assert.equal(pathLabel('~/Documents/claims.pdf', undefined), '~/Documents/claims.pdf');
});
