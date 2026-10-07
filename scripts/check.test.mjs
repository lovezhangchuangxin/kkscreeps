// 验证门禁的进程边界，避免测试选择了旧 release 或把失败当成成功。
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runCheck } from './check.mjs';

test('门禁先构建当前 CLI，覆盖旧 release 与外部二进制选择', () => {
  const calls = [];
  const status = runCheck([], {
    root: '/repo', env: { KKSCREEPS_BIN: '/repo/target/release/kkscreeps', CARGO_TARGET_DIR: '/old-build', KKSCREEPS_SCENARIO: 'one-test' },
    run(command, args, options) { calls.push({ command, args, options }); return { status: 0 }; },
  });
  assert.equal(status, 0);
  const build = calls.findIndex(c => c.command === 'cargo' && c.args[0] === 'build');
  const js = calls.findIndex(c => c.command === 'pnpm');
  assert.ok(build >= 0 && build < js);
  assert.equal(calls[js].options.env.KKSCREEPS_SCENARIO, undefined);
  assert.equal(calls[js].options.env.CARGO_TARGET_DIR, '/repo/target');
  assert.equal(calls[js].options.env.KKSCREEPS_BIN, '/repo/target/debug/kkscreeps');
  assert.ok(calls.some(c => c.args.includes('scripts/check-source-size.mjs')));
});

test('构建失败立即退出，不能继续执行可能跳过引擎的 JS 测试', () => {
  const calls = [];
  const status = runCheck(['kk-world'], { run(command, args) {
    calls.push(command); return { status: args[0] === 'build' ? 7 : 0 };
  } });
  assert.equal(status, 7);
  assert.ok(!calls.includes('pnpm'));
});

test('启动失败及未知参数均返回失败', () => {
  assert.equal(runCheck([], { run: () => ({ status: null, error: new Error('ENOENT') }) }), 1);
  assert.equal(runCheck(['--fix'], { run: () => { throw new Error('不应执行'); } }), 1);
});
