// 适配器冒烟：真实拉起 kkscreeps embed 子进程走一遍 createShard → place → runPlayer → 断言。
// 没有 kkscreeps 二进制时跳过（JS-only 环境，如 CI 的 js job）。

import assert from 'node:assert/strict';
import { existsSync, writeFileSync, mkdtempSync, chmodSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { test } from 'node:test';

const repoRoot = resolve(import.meta.dirname, '../..');
const bin =
  process.env['KKSCREEPS_BIN'] ??
  [resolve(repoRoot, 'target/release/kkscreeps'), resolve(repoRoot, 'target/debug/kkscreeps')].find(
    existsSync,
  );

test(
  'kkscreeps 适配器端到端',
  { skip: bin ? false : '未找到 kkscreeps 二进制（先 cargo build -p kk-cli）' },
  async () => {
    const { createAdapter } = await import('../dist/adapter.js');
    const adapter = await createAdapter();
    try {
      await adapter.createShard({
        players: ['p1'],
        rooms: [{ name: 'W1N1', rcl: 1, owner: 'p1' }],
      });
      const pos = await adapter.getControllerPos('W1N1');
      assert.deepEqual(pos, { x: 1, y: 1 });

      const id = await adapter.placeCreep('W1N1', {
        pos: [25, 25],
        owner: 'p1',
        body: ['move', 'work'],
        store: { energy: 30 },
      });
      const t0 = await adapter.getGameTime();
      const rc = await adapter.runPlayer(
        'p1',
        `Game.getObjectById(${JSON.stringify(id)}).pos.x + Object.keys(Game.creeps).length`,
      );
      assert.equal(rc, 26, 'runPlayer 看见摆放的 creep（x=25 + 1 个 creep）');
      assert.equal(await adapter.getGameTime(), t0 + 1, '恰好推进 1 tick');

      const objs = await adapter.findInRoom('W1N1', 101);
      assert.equal(objs.length, 1, 'findInRoom(FIND_CREEPS)');
    } finally {
      await adapter.teardown();
    }
  },
);

test(
  'tick 的 AbortSignal：每 tick 前检查、中止即抛 reason',
  { skip: bin ? false : '未找到 kkscreeps 二进制（先 cargo build -p kk-cli）' },
  async () => {
    const { createAdapter } = await import('../dist/adapter.js');
    const adapter = await createAdapter();
    try {
      await adapter.createShard({
        players: ['p1'],
        rooms: [{ name: 'W1N1', rcl: 1, owner: 'p1' }],
      });
      // 预中止：立即抛 reason，一个 tick 也不推进
      const c0 = new AbortController();
      c0.abort();
      const t0 = await adapter.getGameTime();
      await assert.rejects(
        adapter.tick(5, { signal: c0.signal }),
        (e) => e === c0.signal.reason,
        '预中止抛 signal.reason 本体',
      );
      assert.equal(await adapter.getGameTime(), t0, '未推进任何 tick');
      // 正常 signal：跑完不抛
      await adapter.tick(2, { signal: new AbortController().signal });
      assert.equal(await adapter.getGameTime(), t0 + 2, 'signal 在途不误伤');
      // 在 tick 序列中中止，验证抛出同一 reason 并停止后续推进。
      // 中止可能发生在 RPC 处理中，因此只要求推进量小于请求数，不固定已完成 tick 数。
      const reason = new Error('围栏超时');
      const c1 = new AbortController();
      setTimeout(() => c1.abort(reason), 0);
      const t1 = await adapter.getGameTime();
      await assert.rejects(
        adapter.tick(1000, { signal: c1.signal }),
        (e) => e === reason,
        '中途中止抛 reason 本体',
      );
      const advanced = (await adapter.getGameTime()) - t1;
      assert.ok(advanced < 1000, `中止后不再推进（已推进 ${advanced}）`);
    } finally {
      await adapter.teardown();
    }
  },
);

test('RPC 超时：死锁 sidecar 被拒绝并 kill（新调用恢复）', async () => {
  const { EmbedClient } = await import('../dist/rpc.js');
  // 假子进程读取请求但不响应，模拟连接一直没有返回结果。
  const dir = mkdtempSync(resolve(tmpdir(), 'kk-rpc-'));
  const hangBin = resolve(dir, 'hang.sh');
  writeFileSync(hangBin, '#!/bin/sh\nexec cat > /dev/null\n');
  chmodSync(hangBin, 0o755);
  const prevBin = process.env['KKSCREEPS_BIN'];
  const prevTimeout = process.env['KKSCREEPS_RPC_TIMEOUT_MS'];
  process.env['KKSCREEPS_BIN'] = hangBin;
  process.env['KKSCREEPS_RPC_TIMEOUT_MS'] = '120';
  const client = new EmbedClient();
  try {
    await assert.rejects(client.call('getGameTime', {}), /RPC 超时/, '超时拒绝');
    // kill 已触发：child 清空，下一次调用尝试重新 spawn（仍挂 → 再次超时）
    await assert.rejects(client.call('getGameTime', {}), /RPC 超时/, 'kill 后可再 spawn');
  } finally {
    client.kill();
    rmSync(dir, { recursive: true, force: true });
    if (prevBin === undefined) delete process.env['KKSCREEPS_BIN'];
    else process.env['KKSCREEPS_BIN'] = prevBin;
    if (prevTimeout === undefined) delete process.env['KKSCREEPS_RPC_TIMEOUT_MS'];
    else process.env['KKSCREEPS_RPC_TIMEOUT_MS'] = prevTimeout;
  }
});
