import assert from 'node:assert/strict';
import { test } from 'node:test';

// 占位冒烟测试：证明 pnpm workspace 的 test 管线可用。
// 包获得实际内容后（M3），换成 Vitest 与真实测试。
test('placeholder package resolves', async () => {
  const pkg = await import('../package.json', { with: { type: 'json' } });
  assert.ok(pkg.default.name.startsWith('@kkscreeps/'));
});
