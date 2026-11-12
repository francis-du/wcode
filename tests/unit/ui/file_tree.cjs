'use strict';
// The file tree is a bounded observation: a large snapshot must not flood the
// DOM, and expanding it must stay an explicit operator action that a new query
// collapses again.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

function largeSnapshot(count) {
  return {
    ...project(),
    structure: {
      entries: Array.from({ length: count }, (_, index) => ({
        path: `src/file-${String(index).padStart(3, '0')}.rs`,
        language: 'rust',
        lines: 10,
      })),
      largest_files: [],
      oversized_files: 0,
      truncated: false,
    },
  };
}

test('the file tree renders a bounded first page and expands only on request', () => {
  const s = sandbox();
  s.context.fixture = largeSnapshot(205);
  s.run('state.project=fixture;renderProjectStructure();');
  const bounded = s.node('#fileTree').innerHTML;
  assert.equal((bounded.match(/data-source-path=/g) || []).length, 200, 'the first page stays bounded');
  assert.match(bounded, /Showing 200 of 205 files/);
  assert.match(bounded, /data-file-tree-show-all/);
  s.run('state.fileTreeExpanded=true;invalidate("fileTree");renderProjectStructure();');
  const expanded = s.node('#fileTree').innerHTML;
  assert.equal((expanded.match(/data-source-path=/g) || []).length, 205, 'expanding renders every entry');
  assert.doesNotMatch(expanded, /data-file-tree-show-all/);
  s.run('els.fileSearch.value="file-204";renderProjectStructure();');
  assert.equal((s.node('#fileTree').innerHTML.match(/data-source-path=/g) || []).length, 1, 'a new query collapses the expansion');
});

test('a snapshot at or below the page size renders without an expansion control', () => {
  const s = sandbox();
  s.context.fixture = largeSnapshot(200);
  s.run('state.project=fixture;renderProjectStructure();');
  const html = s.node('#fileTree').innerHTML;
  assert.equal((html.match(/data-source-path=/g) || []).length, 200);
  assert.doesNotMatch(html, /data-file-tree-show-all/);
  assert.doesNotMatch(html, /Showing 200 of 200 files/);
});

test('the bounded page keeps the localized expansion control', () => {
  for (const [language, label] of [['en', 'Show 5 more files'], ['zh-CN', '显示其余 5 个文件']]) {
    const s = sandbox();
    s.context.fixture = largeSnapshot(205);
    s.context.fixtureLanguage = language;
    s.run('state.language=fixtureLanguage;state.project=fixture;renderProjectStructure();');
    assert.match(s.node('#fileTree').innerHTML, new RegExp(label));
  }
});
