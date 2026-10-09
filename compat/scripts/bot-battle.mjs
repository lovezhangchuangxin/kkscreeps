// 锁定原版 TooAngel 防御战斗验收：真实结算、双投影、跨进程冷恢复与逐位
// 输入回放。场景为单房对峙切片（敌方脚本坦克入侵 + bot 真实防御链），
// 不含多房进攻、第三方 bot 对战或完整 M2B。
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  writeFileSync,
} from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

import { prepareModules } from './bot-gameplay.mjs';

const root = resolve(import.meta.dirname, '../..');
const fixture = join(root, 'compat/fixtures/sim-tooangel-battle');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

// 战斗判定：通用 11 项（对齐 bot-gameplay 的 evaluateRun 通用部分，含
// 双玩家清洁）+ 防御链指标（塔攻击/承受伤害/防御者出生/安全模式）+
// 经济共存下限，另附 5 项 harness 检查（通道/退出码/二进制与模块身份）；
// 前缀段跳过战斗域指标，只验通用与 harness。阈值来自锁定版本的实测
// 观察，见 summary 的 metrics。
function evaluateBattleRun(report, { rounds, totalTicks, startTime, battle = true }) {
  const checks = [];
  const check = (name, passed, actual) =>
    checks.push({ name, passed: passed === true, actual: actual ?? null });
  const objects = report.final?.world?.objects || [];
  const stats = report.final?.players?.bot?.memory?.__kkAcceptance || {};
  const botPlayer = report.players?.bot || {};
  const enemyPlayer = report.players?.enemy || {};
  check('运行成功', report.success === true && report.status === 'passed', report.status);
  check(
    '提交完整轮数',
    report.world_ticks_committed === rounds && report.ticks_attempted === rounds,
    report.world_ticks_committed,
  );
  check(
    'bot 玩家无执行失败或跳帧',
    botPlayer.successful_ticks === rounds &&
      botPlayer.error_ticks === 0 &&
      botPlayer.not_evaluated_ticks === 0,
    botPlayer,
  );
  check(
    '敌方玩家无执行失败或跳帧',
    enemyPlayer.successful_ticks === rounds &&
      enemyPlayer.error_ticks === 0 &&
      enemyPlayer.not_evaluated_ticks === 0,
    enemyPlayer,
  );
  check('世界时间接续', report.start_time === startTime && report.end_time === startTime + rounds, [
    report.start_time,
    report.end_time,
  ]);
  check('公开观察 Memory 跨恢复接续', stats.loops === totalTicks, stats.loops);
  check(
    '上一拍事件观察窗口完整',
    stats.eventsThroughTick === totalTicks - 1,
    stats.eventsThroughTick,
  );
  check('bot 未记录捕获异常', stats.caughtErrors === 0, stats.errorSamples);
  check(
    '输入日志完整',
    report.input_log?.validation === 'ok' &&
      report.input_log?.status !== 'detached' &&
      report.input_log?.segments === totalTicks &&
      report.input_log?.read_back_segments === totalTicks &&
      report.input_log?.first_tick === 1 &&
      report.input_log?.last_tick === totalTicks,
    report.input_log,
  );
  check(
    '整份检查点输入回放相等',
    report.verify_replay?.status === 'passed' && report.verify_replay?.segments === rounds,
    report.verify_replay,
  );
  check(
    '场景断言通过',
    Array.isArray(report.assertions) &&
      report.assertions.length > 0 &&
      report.assertions.every((a) => a.passed === true),
    report.assertions,
  );
  // ── 防御链指标（仅完整窗口段：前缀段截在防御腿触发之前）──
  if (battle) {
    check('初始安全模式额度为 1', stats.safeModeAvailableTick0 === 1, stats.safeModeAvailableTick0);
    check(
      '对敌攻击事件实际发生',
      Number.isFinite(stats.attackEvents) && stats.attackEvents >= 50,
      stats.attackEvents,
    );
    check(
      '塔对敌 creep 实际攻击',
      Number.isFinite(stats.towerAttackEvents) && stats.towerAttackEvents >= 50,
      stats.towerAttackEvents,
    );
    check(
      '敌方 creep 实际承伤',
      Number.isFinite(stats.raiderDamage) && stats.raiderDamage >= 500,
      stats.raiderDamage,
    );
    check(
      '敌方 creep 实际自疗',
      Number.isFinite(stats.raiderHealed) && stats.raiderHealed >= 300,
      stats.raiderHealed,
    );
    check('防御者实际出生', Number.isFinite(stats.combatBirths) && stats.combatBirths >= 1, {
      combatBirths: stats.combatBirths,
      firstTick: stats.defenderFirstTick,
    });
    check(
      '安全模式实际激活',
      Number.isFinite(stats.safeModeTick) && stats.safeModeTick !== null && stats.safeModeTick > 1,
      stats.safeModeTick,
    );
    const controller = report.final?.rooms?.W1N1?.controller;
    // 快照 safeMode 是剩余 tick 数而非绝对截止拍，> 0 即激活中；
    // 激活时点在窗内由观察器 safeModeTick 断言。
    check(
      '终态安全模式生效且所有权保持',
      controller?.owner === 'bot' &&
        Number.isFinite(controller?.safeMode) &&
        controller.safeMode > 0,
      controller,
    );
    // ── 战斗期间经济共存下限 ──
    check(
      '战斗期间经济未停摆',
      Number.isFinite(stats.harvested) && stats.harvested >= 200,
      stats.harvested,
    );
    check(
      '终态 spawn 存活',
      objects.some((o) => o.structureType === 'spawn' && o.owner === 'bot'),
      objects.filter((o) => o.structureType === 'spawn').length,
    );
  }
  return { success: checks.every((c) => c.passed), checks, metrics: stats };
}

// 默认窗口即验证窗口（README 同口径）：防御链三腿 2/394/399 落在窗内，
// defendmelee 与敌方坦克同格的 bot 潜在崩溃点 556 在窗外。
export function runBattleAcceptance({ out, binary, botRoot, ticks = 520, split = 260 }) {
  botRoot ||= join(root, 'compat/fixtures/tooangel');
  if (
    !Number.isSafeInteger(ticks) ||
    ticks < 400 ||
    !Number.isSafeInteger(split) ||
    split < 1 ||
    split >= ticks
  ) {
    throw new Error('ticks 须 ≥400；split 须为 1..ticks-1 的整数');
  }
  out = join(realpathSync(dirname(resolve(out))), basename(out));
  for (const source of [botRoot, fixture]) {
    const path = relative(realpathSync(source), out);
    if (path === '' || (path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path))) {
      throw new Error('输出目录不得位于 bot 或验收夹具源码内');
    }
  }
  // 输出目录必须全新；模块身份核验后才写，禁止复用旧成功报告。
  mkdirSync(out, { recursive: false });
  const summary = {
    version: 1,
    success: false,
    scope:
      'TooAngel 单房防御战斗/冷恢复（敌方脚本坦克 + 真实防御链）；不含多房进攻、bot 对战或完整 M2B',
    bot: 'tooangel',
    ticks,
    split,
    runs: [],
  };
  try {
    summary.executable_sha256 = sha256(readFileSync(binary));
    // 观察器随 prepareModules 以 kk_battle 模块身份部署；模块目录只含
    // 锁定 bot 源码与该观察器，不混入经济夹具的 acceptance.js。
    summary.upstream = prepareModules(botRoot, join(out, 'modules'), {
      source: join(fixture, 'observer.js'),
      target: 'kk_battle.js',
    });
    summary.observer_sha256 = {
      file: 'observer.js',
      sha256: summary.upstream.observer_sha256,
    };
    const scenario = join(out, 'scenario.json');
    copyFileSync(join(fixture, 'scenario.json'), scenario);
    mkdirSync(join(out, 'enemy'));
    for (const name of readdirSync(join(fixture, 'enemy'))) {
      copyFileSync(join(fixture, 'enemy', name), join(out, 'enemy', name));
    }
    summary.enemy_sha256 = sha256(readFileSync(join(out, 'enemy', 'main.js')));
    const run = (name, channel, rounds, extra, totalTicks, startTime, battle = true) => {
      const reportPath = join(out, `${name}.json`);
      const args = [
        'sim',
        'run',
        '--scenario',
        scenario,
        '--ticks',
        String(rounds),
        '--concurrency',
        '1',
        '--report',
        reportPath,
        '--verify-replay',
        ...extra,
      ];
      if (channel === 'kkpv') args.push('--kkpv');
      let exitCode = 0;
      try {
        execFileSync(binary, args, {
          cwd: root,
          timeout: 60000,
          stdio: ['ignore', 'pipe', 'pipe'],
        });
      } catch (error) {
        exitCode = error.status ?? 1;
      }
      const report = JSON.parse(readFileSync(reportPath, 'utf8'));
      const result = evaluateBattleRun(report, { rounds, totalTicks, startTime, battle });
      // 报告内模块身份元数据须与 summary 记录的哈希一致，闭合
      // observer/enemy 只记录不比对的身份链。
      const moduleSha = (side, name) =>
        report.modules?.[side]?.files?.find((file) => file.name === name)?.sha256;
      result.checks.push(
        { name: '实际投影通道', passed: report.channel === channel, actual: report.channel },
        { name: 'CLI 成功退出', passed: exitCode === 0, actual: exitCode },
        {
          name: 'CLI 二进制身份一致',
          passed: report.executable_sha256 === summary.executable_sha256,
          actual: report.executable_sha256,
        },
        {
          name: '观察器模块身份一致',
          passed: moduleSha('bot', 'kk_battle') === summary.observer_sha256.sha256,
          actual: moduleSha('bot', 'kk_battle'),
        },
        {
          name: '敌方脚本身份一致',
          passed: moduleSha('enemy', 'main') === summary.enemy_sha256,
          actual: moduleSha('enemy', 'main'),
        },
      );
      result.success = result.checks.every((check) => check.passed);
      summary.runs.push({
        name,
        channel,
        exit_code: exitCode,
        report: basename(reportPath),
        executable_sha256: report.executable_sha256,
        ...result,
      });
      console.log(`${name}: ${result.success ? 'passed' : 'failed'}`);
      if (!result.success) throw new Error(`${name} 未通过；查看逐项 checks 与 sim 报告`);
    };
    for (const channel of ['json', 'kkpv']) {
      const log = join(out, `${channel}-resume.jsonl`);
      const checkpoint = join(out, `${channel}-checkpoint.json`);
      run(
        `${channel}-continuous`,
        channel,
        ticks,
        ['--input-log', join(out, `${channel}-continuous.jsonl`)],
        ticks,
        1,
      );
      run(
        `${channel}-prefix`,
        channel,
        split,
        ['--input-log', log, '--save-checkpoint', checkpoint],
        split,
        1,
        false,
      );
      run(
        `${channel}-resumed`,
        channel,
        ticks - split,
        ['--input-log', log, '--load-checkpoint', checkpoint],
        ticks,
        split + 1,
      );
    }
    summary.success = true;
  } catch (error) {
    summary.error = error.message;
  }
  writeFileSync(join(out, 'summary.json'), JSON.stringify(summary, null, 2) + '\n');
  return summary;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const options = {
      binary: resolve(process.env.KKSCREEPS_BIN || join(root, 'target/release/kkscreeps')),
    };
    const names = {
      '--out': 'out',
      '--bin': 'binary',
      '--bot-root': 'botRoot',
      '--ticks': 'ticks',
      '--split': 'split',
    };
    const seen = new Set();
    for (let i = 2; i < process.argv.length; i += 2) {
      const key = names[process.argv[i]],
        value = process.argv[i + 1];
      if (!key || seen.has(key) || !value || value.startsWith('--'))
        throw new Error('参数须为 --out/--bin/--bot-root/--ticks/--split 的唯一键值对');
      seen.add(key);
      options[key] = key === 'ticks' || key === 'split' ? Number(value) : resolve(value);
    }
    if (!options.out || !isAbsolute(options.out))
      throw new Error('须用 --out 指定不存在的输出目录（父目录须已存在）');
    const summary = runBattleAcceptance(options);
    console.log(`报告：${join(options.out, 'summary.json')}`);
    process.exitCode = summary.success ? 0 : 2;
    if (summary.error) console.error(summary.error);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
