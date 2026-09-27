// 适配器冒烟：真实拉起 kkscreeps embed 子进程走一遍 createShard → place → runPlayer → 断言。
// 没有 kkscreeps 二进制时跳过（JS-only 环境，如 CI 的 js job）。

import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

const repoRoot = resolve(import.meta.dirname, '../..');
const bin =
  process.env['KKSCREEPS_BIN'] ??
  [resolve(repoRoot, 'target/release/kkscreeps'), resolve(repoRoot, 'target/debug/kkscreeps')].find(existsSync);

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
