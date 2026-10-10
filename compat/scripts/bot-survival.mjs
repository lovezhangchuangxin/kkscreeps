// 锁定原版 TooAngel 长期存活验收：真实结算、双投影、跨进程冷恢复与
// 逐位输入回放。场景为单房切片（bot 从 RCL1 正确布局锚点自举，86400
// 拍 ≥72h 等价长窗），验证长期经济自持、世代更替、等级爬升、观察器
// Memory 有界与零错误零跳帧；不含 bot 对战、多房扩张运营或完整 M2B。
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

import { evaluateRun, prepareModules } from './bot-gameplay.mjs';

const root = resolve(import.meta.dirname, '../..');
const fixture = join(root, 'compat/fixtures/sim-tooangel-survival');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

// 官方 Memory 序列化上限为 2MB；观察器采样的是剔除 __kkAcceptance 后
// 的 bot 自身 Memory，断言取 256KB（1/8 上限）作为"长期增长远离上限"
// 的裕度线，实测约 30KB 级。
const MEMORY_BOUND_BYTES = 256 * 1024;

// 长期存活判定：通用项复用 bot-gameplay 的 evaluateRun（economy=false，
// 单玩家）+ 存活域指标（世代更替/等级爬升/关键建筑/经济累计/采样有界）
// + 4 项 harness 检查；前缀段跳过存活域指标，只验通用与 harness。
// 阈值来自锁定版本全长窗口的实测观察（见 summary 的 metrics），
// 均为留有裕度的下限：跨进程冷恢复会重置 TooAngel 的 heap 态
// （resourceStats EMA 重热身）， resumed 段的脑层节奏会晚于连跑段。
function evaluateSurvivalRun(report, { rounds, totalTicks, startTime, survival = true }) {
  const result = evaluateRun(report, { ticks: rounds, totalTicks, startTime, economy: false });
  const checks = result.checks;
  const check = (name, passed, actual) =>
    checks.push({ name, passed: passed === true, actual: actual ?? null });
  const objects = report.final?.world?.objects || [];
  const stats = result.metrics;
  if (survival) {
    const controller = report.final?.rooms?.W1N1?.controller;
    check('控制器等级实际爬升', (stats.maxControllerLevel || 0) >= 4, stats.maxControllerLevel);
    check(
      '控制器等级终态与峰值一致且所有权保持',
      controller?.level === stats.maxControllerLevel && controller?.owner === 'bot',
      controller,
    );
    check(
      '关键建筑实际建成',
      ['extension', 'rampart', 'tower', 'storage'].every(
        (type) => stats.structureFirstTicks?.[type] !== undefined,
      ),
      stats.structureFirstTicks,
    );
    check(
      'storage 后经济角色实际上岗',
      (stats.roleBirths?.storagefiller || 0) >= 5 && (stats.roleBirths?.upgrader || 0) >= 5,
      { storagefiller: stats.roleBirths?.storagefiller, upgrader: stats.roleBirths?.upgrader },
    );
    check('多代 creep 实际更替', stats.births >= 300 && stats.deaths >= 100, {
      births: stats.births,
      deaths: stats.deaths,
    });
    check(
      '终态存活 creep 维持且有界',
      objects.filter((o) => o.type === 'creep' && o.owner === 'bot' && !o.spawning && o.hits > 0)
        .length >= 6 && objects.filter((o) => o.type === 'creep').length <= 40,
      objects.filter((o) => o.type === 'creep').length,
    );
    check('scout 周期性实际出生', (stats.roleBirths?.scout || 0) >= 5, stats.roleBirths?.scout);
    check(
      '经济持续运转（采集）',
      Number.isFinite(stats.harvested) && stats.harvested >= 1_000_000,
      stats.harvested,
    );
    check(
      '经济持续运转（转运）',
      Number.isFinite(stats.transferred) && stats.transferred >= 1_000_000,
      stats.transferred,
    );
    // built 下限按全长实测 154,584 取 100k：路墙主体建成后建造量收敛。
    check('建造持续进行', Number.isFinite(stats.built) && stats.built >= 100_000, stats.built);
    check(
      '控制器持续升级投入',
      Number.isFinite(stats.upgraded) && stats.upgraded >= 400_000,
      stats.upgraded,
    );
    check('CPU 桶零跳帧', stats.cpuSkips === 0, stats.cpuSkips);
    const samples = stats.worldSamples || [];
    check(
      'Memory 采样曲线完整',
      samples.length >= Math.floor(totalTicks / 500) - 2,
      samples.length,
    );
    check(
      '观察器外 Memory 字节数远离官方 2MB 上限',
      samples.length > 0 &&
        samples.every(([, bytes]) => Number.isFinite(bytes) && bytes <= MEMORY_BOUND_BYTES),
      samples.length ? [samples[0][1], samples[samples.length - 1][1]] : null,
    );
    check(
      '终态 spawn 存活',
      objects.some((o) => o.structureType === 'spawn' && o.owner === 'bot'),
      objects.filter((o) => o.structureType === 'spawn').length,
    );
  }
  return { success: checks.every((c) => c.passed), checks, metrics: stats };
}

// 默认窗口即验证窗口（README 同口径）：≥72h 等价长窗（官方 1 tick≈3s）
// 取 86400 拍；截点 43200 落在 storage 建成与 EMA 预热之后，使冷恢复
// 覆盖成熟房运营段。夹具 spawn 位于 TooAngel 布局锚点 (5,5)（控制器
// 自动放置于 (1,1)），misplacedSpawn 不触发、isHealthy 可成立；kk 为
// 单房夹具播种 GCL2，brain 层 nextroomer 过 GCL 门后卡在无可占的已侦察
// 房（scout 冻结于全墙边缘的既知约束），退化为周期性 scout 出生。
export function runSurvivalAcceptance({ out, binary, botRoot, ticks = 86400, split = 43200 }) {
  botRoot ||= join(root, 'compat/fixtures/tooangel');
  if (
    !Number.isSafeInteger(ticks) ||
    ticks < 86400 ||
    !Number.isSafeInteger(split) ||
    split < 1 ||
    split >= ticks
  ) {
    throw new Error('ticks 须 ≥86400；split 须为 1..ticks-1 的整数');
  }
  out = join(realpathSync(dirname(resolve(out))), basename(out));
  for (const source of [botRoot, fixture]) {
    const path = relative(realpathSync(source), out);
    if (path === '' || (path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path))) {
      throw new Error('输出目录不得位于 bot 或验收夹具源码内');
    }
  }
  // 输出目录必须全新；模块身份核验后才写，禁止复用旧成功报告。
  // 全矩阵输入日志约 10GB（30KB/拍），运行目录须预留磁盘。
  mkdirSync(out, { recursive: false });
  const summary = {
    version: 1,
    success: false,
    scope:
      'TooAngel 单房长期存活/冷恢复（≥72h 等价 86400 拍，RCL1 布局锚点自举）；不含 bot 对战、多房扩张运营或完整 M2B',
    bot: 'tooangel',
    ticks,
    split,
    runs: [],
  };
  try {
    summary.executable_sha256 = sha256(readFileSync(binary));
    // 观察器随 prepareModules 以 kk_survival 模块身份部署；模块目录只含
    // 锁定 bot 源码与该观察器，不混入经济夹具的 acceptance.js。
    summary.upstream = prepareModules(botRoot, join(out, 'modules'), {
      source: join(fixture, 'observer.js'),
      target: 'kk_survival.js',
    });
    summary.observer_sha256 = {
      file: 'observer.js',
      sha256: summary.upstream.observer_sha256,
    };
    const scenario = join(out, 'scenario.json');
    copyFileSync(join(fixture, 'scenario.json'), scenario);
    const run = (name, channel, rounds, extra, totalTicks, startTime, survival = true) => {
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
        // 86400 拍单段实测约 10-20 分钟（含逐位回放）；留量到 60 分钟。
        execFileSync(binary, args, {
          cwd: root,
          timeout: 3600000,
          stdio: ['ignore', 'pipe', 'pipe'],
        });
      } catch (error) {
        exitCode = error.status ?? 1;
      }
      const report = JSON.parse(readFileSync(reportPath, 'utf8'));
      const result = evaluateSurvivalRun(report, { rounds, totalTicks, startTime, survival });
      // 报告内模块身份元数据须与 summary 记录的哈希一致，闭合观察器
      // 只记录不比对的身份链。
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
          passed: moduleSha('bot', 'kk_survival') === summary.observer_sha256.sha256,
          actual: moduleSha('bot', 'kk_survival'),
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
    const summary = runSurvivalAcceptance(options);
    console.log(`报告：${join(options.out, 'summary.json')}`);
    process.exitCode = summary.success ? 0 : 2;
    if (summary.error) console.error(summary.error);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
