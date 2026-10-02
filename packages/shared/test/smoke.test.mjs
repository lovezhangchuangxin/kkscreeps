import assert from 'node:assert/strict';
import { test } from 'node:test';

// 仅检查 package.json 能被导入且包名使用项目命名空间，不覆盖实际功能。
test('placeholder package resolves', async () => {
  const pkg = await import('../package.json', { with: { type: 'json' } });
  assert.ok(pkg.default.name.startsWith('@kkscreeps/'));
});
