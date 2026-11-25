import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const translations = new Map(readFileSync(new URL('../shell/i18n/zh-CN.tsv', import.meta.url), 'utf8')
  .split(/\r?\n/)
  .filter(line => line.trim() && !line.startsWith('#'))
  .map(line => line.split('\t').slice(0, 2).map(value => value
    .replace(/\\n/g, '\n').replace(/\\t/g, '\t').replace(/\\\\/g, '\\'))));

test('missing-file message keeps its path and newline in the source key', () => {
  assert.equal(translations.get('{path}\nnot found'), '{path}\n未找到');
});

test('plural summary does not acquire a file path from the following row', () => {
  assert.equal(translations.get('{n} {what}s'), '{n} {what}');
});

test('translations preserve the source formatting placeholders', () => {
  const placeholders = text => [...text.matchAll(/\{[A-Za-z_][^{}]*\}/g)]
    .map(match => match[0]).sort();
  for (const [source, translated] of translations) {
    assert.deepEqual(placeholders(translated), placeholders(source), source);
  }
});
