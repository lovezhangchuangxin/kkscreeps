// 文档门禁只检查仓库内容，并包含尚未暂存的新文档。
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { collectMarkdown, checkDocLinks } from './check-doc-links.mjs';

test('忽略工作副本、不依赖暂存，并跳过已删除的跟踪文件', () => {
  const root = mkdtempSync(join(tmpdir(), 'kk-doc-links-'));
  try {
    execFileSync('git', ['init', '-q', root]);
    writeFileSync(join(root, '.gitignore'), '.worktrees/\n');
    mkdirSync(join(root, '.worktrees'));
    writeFileSync(join(root, '.worktrees', 'old.md'), '[bad](missing.md)');
    writeFileSync(join(root, 'removed.md'), '# Old');
    execFileSync('git', ['add', 'removed.md'], { cwd: root });
    rmSync(join(root, 'removed.md'));
    writeFileSync(join(root, 'new.md'), '# New\n[missing](target.md)');
    assert.deepEqual(collectMarkdown(root), [join(root, 'new.md')]);
    assert.equal(checkDocLinks(root), 1);
    writeFileSync(join(root, 'target.md'), '# Target');
    assert.equal(checkDocLinks(root), 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
