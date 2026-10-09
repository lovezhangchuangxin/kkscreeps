// 防止固定世界、空回放和 bot 自吞异常被新验收入口计为通过。
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import vm from 'node:vm';
import { evaluateRun, runAcceptance } from '../scripts/bot-gameplay.mjs';

const expected = { ticks: 1000, totalTicks: 2000, startTime: 1001 };
function passingReport() {
  return {
    success: true,
    status: 'passed',
    world_ticks_committed: 1000,
    ticks_attempted: 1000,
    start_time: 1001,
    end_time: 2001,
    players: { bot: { successful_ticks: 1000, error_ticks: 0, not_evaluated_ticks: 0 } },
    input_log: {
      validation: 'ok',
      segments: 2000,
      read_back_segments: 2000,
      first_tick: 1,
      last_tick: 2000,
    },
    verify_replay: { status: 'passed', segments: 1000 },
    assertions: [{ passed: true }],
    initial: {
      world: { objects: [{ type: 'structure', structureType: 'spawn' }] },
      players: { bot: { gcl: 0 } },
    },
    final: {
      world: {
        objects: [
          { type: 'creep', owner: 'bot', hits: 100, spawning: false },
          { type: 'creep', owner: 'bot', hits: 100, spawning: false },
          { type: 'structure', structureType: 'extension', owner: 'bot' },
        ],
      },
      rooms: { W1N1: { controller: { level: 2, owner: 'bot' } } },
      players: {
        bot: {
          gcl: 200,
          memory: {
            __kkAcceptance: {
              loops: 2000,
              eventsThroughTick: 1999,
              births: 2,
              birthsAfterDeath: 1,
              deaths: 1,
              harvested: 1000,
              transferred: 200,
              built: 200,
              upgraded: 200,
              caughtErrors: 0,
              errorSamples: [],
            },
          },
        },
      },
    },
  };
}

test('恢复验收区分本次运行段数与整条日志段数', () => {
  assert.equal(evaluateRun(passingReport(), expected).success, true);
});

test('成功退出不能遮蔽空玩法、失效恢复、漏记输入或失败回放', () => {
  const mutations = [
    (r) => {
      r.final.players.bot.memory.__kkAcceptance.harvested = 0;
    },
    (r) => {
      r.final.players.bot.memory.__kkAcceptance.deaths = 0;
    },
    (r) => {
      r.final.players.bot.memory.__kkAcceptance.birthsAfterDeath = 0;
    },
    (r) => {
      r.final.players.bot.memory.__kkAcceptance.loops = 1000;
    },
    (r) => {
      r.final.players.bot.memory.__kkAcceptance.caughtErrors = 1;
    },
    (r) => {
      r.final.world.objects[2].structureType = 'constructionSite';
    },
    (r) => {
      r.initial.world.objects.push({ type: 'creep' });
    },
    (r) => {
      r.final.rooms.W1N1.controller.owner = 'other';
    },
    (r) => {
      r.final.players.bot.gcl = 199;
    },
    (r) => {
      r.input_log.status = 'detached';
    },
    (r) => {
      r.input_log.read_back_segments = 1000;
    },
    (r) => {
      r.verify_replay.segments = 0;
    },
    (r) => {
      r.verify_replay.status = 'failed';
    },
    (r) => {
      r.players.bot.not_evaluated_ticks = 1;
    },
    (r) => {
      r.assertions = [];
    },
    (r) => {
      delete r.final;
    },
  ];
  for (const mutate of mutations) {
    const report = passingReport();
    mutate(report);
    assert.equal(evaluateRun(report, expected).success, false);
  }
});

const observer = readFileSync(
  resolve(import.meta.dirname, '../fixtures/sim-tooangel/acceptance.js'),
  'utf8',
);
function sandbox(memory, loop, events = []) {
  const context = {
    module: { exports: {} },
    Memory: memory,
    Game: {
      time: 1,
      rooms: { W1N1: { getEventLog: () => events } },
      creeps: { a: { id: 'a', spawning: false }, ghost: { id: 'ghost', spawning: true } },
    },
    EVENT_HARVEST: 1,
    EVENT_TRANSFER: 2,
    EVENT_BUILD: 3,
    EVENT_UPGRADE_CONTROLLER: 4,
    EVENT_OBJECT_DESTROYED: 5,
    console: { log() {} },
    require(name) {
      assert.equal(name, 'main');
      return { loop };
    },
  };
  vm.runInNewContext(observer, context);
  return context;
}

test('公开观察只计正量结算、实际出生和死亡，恢复后不重复计出生', () => {
  const memory = {};
  const first = sandbox(memory, () => {}, [
    { event: 1, data: { amount: 2 } },
    { event: 2, data: { amount: 50 } },
    { event: 3, data: { amount: 0 } },
    { event: 5, data: { type: 'creep' } },
  ]);
  first.module.exports.loop();
  const stats = memory.__kkAcceptance;
  assert.equal(stats.births, 1);
  assert.equal(stats.birthsAfterDeath, 1);
  assert.equal(stats.deaths, 1);
  assert.equal(stats.harvested, 2);
  assert.equal(stats.transferred, 50);
  assert.equal(stats.built, 0);
  // 新 JS 全局复用检查点 Memory；seen 不依赖上次沙箱的模块全局。
  sandbox(memory, () => {}).module.exports.loop();
  assert.equal(stats.births, 1);
  assert.equal(stats.loops, 2);
});

test('bot 捕获后在第 20 条之外打印的异常也使验收失败，并恢复 console', () => {
  let context;
  context = sandbox({}, () => {
    for (let i = 0; i < 25; i++) context.console.log('ordinary log');
    context.console.log('123 W1N1 Executing room failed: TypeError: broken');
  });
  const original = context.console.log;
  assert.throws(() => context.module.exports.loop(), /TooAngel caught exception/);
  assert.equal(context.Memory.__kkAcceptance.caughtErrors, 1);
  assert.equal(context.console.log, original);
});

test('拒绝向源码写入，包括通过父目录符号链接绕入源码', () => {
  const path = mkdtempSync(join(tmpdir(), 'kkscreeps-gameplay-path-'));
  try {
    const source = resolve(import.meta.dirname, '../fixtures/sim-tooangel');
    symlinkSync(source, join(path, 'source'), 'dir');
    assert.throws(
      () =>
        runAcceptance({
          out: join(path, 'source/new-output'),
          botRoot: source,
          binary: '/missing',
        }),
      /不得位于/,
    );
    assert.throws(
      () => runAcceptance({ out: join(path, 'new-output'), ticks: 100, binary: '/missing' }),
      /ticks 须/,
    );
  } finally {
    rmSync(path, { recursive: true, force: true });
  }
});

test('既有输出目录和成功报告不能被复用或覆盖', () => {
  const path = mkdtempSync(join(tmpdir(), 'kkscreeps-gameplay-existing-'));
  try {
    const report = join(path, 'summary.json');
    writeFileSync(report, 'original successful report');
    const source = resolve(import.meta.dirname, '../fixtures/sim-tooangel');
    assert.throws(
      () => runAcceptance({ out: path, botRoot: source, binary: '/missing' }),
      /EEXIST/,
    );
    assert.equal(readFileSync(report, 'utf8'), 'original successful report');
  } finally {
    rmSync(path, { recursive: true, force: true });
  }
});
