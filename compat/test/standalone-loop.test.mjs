// 独立 Node 进程的事件循环回归：pending RPC 必须把进程撑到响应到达。
// 历史缺陷：子进程/管道/超时计时器全 unref ⇒ 顶层 await 的独立脚本在响应
// 到达前事件循环掏空退出（exit 13）；vitest 环境被运行器句柄掩盖。
// 用 spawn 出的全新 node 进程复现真实独立环境（不受本测试运行器影响）。

import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

const repoRoot = resolve(import.meta.dirname, '../..');

test('独立进程：pending RPC 保持事件循环到响应到达', () => {
  // 假 sidecar：吃掉请求行，延迟 300ms 再回包——给宿主事件循环留出"被掏空"
  // 的窗口（响应即时回写时竞态不定，延迟使缺陷确定性显形）
  const dir = mkdtempSync(resolve(tmpdir(), 'kk-standalone-'));
  const slowBin = resolve(dir, 'slow-sidecar.sh');
  writeFileSync(
    slowBin,
    '#!/bin/sh\nhead -n 1 > /dev/null\nsleep 0.3\necho \'{"id":1,"ok":true,"result":123}\'\n',
  );
  chmodSync(slowBin, 0o755);

  const script = `
    import { EmbedClient } from ${JSON.stringify(resolve(repoRoot, 'compat/dist/rpc.js'))};
    const c = new EmbedClient();
    const r = await c.call('getGameTime', {});
    if (r !== 123) throw new Error('bad result: ' + JSON.stringify(r));
    console.log('OK');
  `;
  try {
    const run = spawnSync(process.execPath, ['--input-type=module', '-e', script], {
      encoding: 'utf8',
      env: { ...process.env, KKSCREEPS_BIN: slowBin },
      timeout: 20_000,
    });
    assert.equal(
      run.status,
      0,
      `独立进程应等到 RPC 返回（exit=${run.status}，stdout=${run.stdout}，stderr=${(run.stderr ?? '').slice(0, 400)}）`,
    );
    assert.match(run.stdout, /OK/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
