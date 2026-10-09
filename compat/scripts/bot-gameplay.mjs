// 锁定原版 bot 单房经济验收：真实结算、双投影、跨进程冷恢复与逐位输入回放。
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

import { prepareOvermindModules } from './overmind-gameplay.mjs';

export const TOOANGEL_PIN = '87645a0b933360224737cca3b18ae9d112f46fef';
const root = resolve(import.meta.dirname, '../..');
const fixture = join(root, 'compat/fixtures/sim-tooangel');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

// 仅复制锁定提交的 src/*.js；校验 git blob 与目录清单，不能信根目录旧副本。
// observer 可选 { source, target }：随模块部署的观察器，默认经济夹具的
// acceptance.js；其它验收（如战斗）传自己的观察器，避免无关模块进入
// 模块目录与报告身份元数据。
export function prepareModules(botRoot, destination, observer) {
  observer ||= { source: join(fixture, 'acceptance.js'), target: 'acceptance.js' };
  const git = (...args) =>
    execFileSync('git', ['-C', botRoot, ...args], { encoding: 'utf8' }).trim();
  if (git('rev-parse', 'HEAD') !== TOOANGEL_PIN) throw new Error('TooAngel HEAD 不符合锁定版本');
  const entries = git('ls-tree', '-r', 'HEAD', '--', 'src')
    .split('\n')
    .map((line) => {
      const [meta, name] = line.split('\t');
      return { name, blob: meta.split(' ')[2] };
    })
    .filter(({ name }) => name?.endsWith('.js'));
  if (!entries.length || entries.some(({ name }) => !/^src\/[^/]+\.js$/.test(name))) {
    throw new Error('TooAngel src 模块布局不符合扁平部署约定');
  }
  const actual = readdirSync(join(botRoot, 'src'))
    .filter((name) => name.endsWith('.js'))
    .sort();
  const expected = entries.map(({ name }) => name.slice(4)).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error('TooAngel src 存在缺失或额外模块');
  const files = entries.map(({ name, blob }) => {
    const bytes = readFileSync(join(botRoot, name));
    const actualBlob = createHash('sha1')
      .update(`blob ${bytes.length}\0`)
      .update(bytes)
      .digest('hex');
    if (actualBlob !== blob) throw new Error(`TooAngel 源码与锁定提交不符：${name}`);
    return { name: name.slice(4), bytes, sha256: sha256(bytes) };
  });
  // 全部身份核验完成后才写输出，不覆盖 bot 源码。
  mkdirSync(destination);
  for (const { name, bytes } of files) writeFileSync(join(destination, name), bytes);
  copyFileSync(observer.source, join(destination, observer.target));
  return {
    commit: TOOANGEL_PIN,
    source: resolve(botRoot, 'src'),
    files: files.map(({ name, sha256 }) => ({ name, sha256 })),
    observer_sha256: sha256(readFileSync(observer.source)),
  };
}

export function evaluateRun(report, { ticks, totalTicks, startTime, economy = true }) {
  const checks = [];
  const check = (name, passed, actual) =>
    checks.push({ name, passed: passed === true, actual: actual ?? null });
  const objects = report.final?.world?.objects || [];
  const stats = report.final?.players?.bot?.memory?.__kkAcceptance || {};
  const player = report.players?.bot || {};
  check('运行成功', report.success === true && report.status === 'passed', report.status);
  check(
    '提交完整轮数',
    report.world_ticks_committed === ticks && report.ticks_attempted === ticks,
    report.world_ticks_committed,
  );
  check(
    '玩家无执行失败或跳帧',
    player.successful_ticks === ticks &&
      player.error_ticks === 0 &&
      player.not_evaluated_ticks === 0,
    player,
  );
  check('世界时间接续', report.start_time === startTime && report.end_time === startTime + ticks, [
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
    report.verify_replay?.status === 'passed' && report.verify_replay?.segments === ticks,
    report.verify_replay,
  );
  check(
    '场景断言通过',
    Array.isArray(report.assertions) &&
      report.assertions.length > 0 &&
      report.assertions.every((a) => a.passed === true),
    report.assertions,
  );
  if (economy) {
    const initialObjects = report.initial?.world?.objects || [];
    check(
      '初始无 creep 或预建 extension',
      initialObjects.length > 0 &&
        !initialObjects.some((o) => o.type === 'creep' || o.structureType === 'extension'),
      initialObjects.length,
    );
    for (const [key, minimum] of Object.entries({
      births: 2,
      deaths: 1,
      birthsAfterDeath: 1,
      harvested: 1000,
      transferred: 200,
      built: 200,
      upgraded: 200,
    })) {
      check(
        `实际结算/观察 ${key}`,
        Number.isFinite(stats[key]) && stats[key] >= minimum,
        stats[key],
      );
    }
    check(
      '终态至少两只已出生 creep 存活',
      objects.filter((o) => o.type === 'creep' && o.owner === 'bot' && !o.spawning && o.hits > 0)
        .length >= 2,
      objects.filter((o) => o.type === 'creep').length,
    );
    check(
      '实际建成 extension',
      objects.some((o) => o.structureType === 'extension' && o.owner === 'bot'),
      objects.filter((o) => o.structureType === 'extension').length,
    );
    check(
      '控制器升级并保持所有权',
      report.final?.rooms?.W1N1?.controller?.level >= 2 &&
        report.final?.rooms?.W1N1?.controller?.owner === 'bot',
      report.final?.rooms?.W1N1?.controller,
    );
    const delta = report.final?.players?.bot?.gcl - report.initial?.players?.bot?.gcl;
    check('账户 GCL 实际增长', Number.isFinite(delta) && delta >= 200, delta);
  }
  return { success: checks.every((c) => c.passed), checks, metrics: stats };
}

export function runAcceptance({
  out,
  binary,
  bot = 'tooangel',
  botRoot,
  ticks = 2000,
  split = 1000,
}) {
  if (!['tooangel', 'overmind'].includes(bot)) throw new Error('bot 须为 tooangel 或 overmind');
  botRoot ||= join(root, `compat/fixtures/${bot === 'tooangel' ? 'tooangel' : 'overmind-dist'}`);
  const selectedFixture = bot === 'tooangel' ? fixture : join(root, 'compat/fixtures/sim-overmind');
  const prepare = bot === 'tooangel' ? prepareModules : prepareOvermindModules;
  const label = bot === 'tooangel' ? 'TooAngel' : 'Overmind';
  if (
    !Number.isSafeInteger(ticks) ||
    ticks < 2000 ||
    !Number.isSafeInteger(split) ||
    split < 1 ||
    split >= ticks
  ) {
    throw new Error('ticks 须 ≥2000；split 须为 1..ticks-1 的整数');
  }
  out = join(realpathSync(dirname(resolve(out))), basename(out));
  for (const source of [botRoot, selectedFixture]) {
    const path = relative(realpathSync(source), out);
    if (path === '' || (path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path))) {
      throw new Error('输出目录不得位于 bot 或验收夹具源码内');
    }
  }
  // 输出目录必须全新；运行与检查点共用模块身份，禁止复用旧成功报告。
  mkdirSync(out, { recursive: false });
  const summary = {
    version: 1,
    success: false,
    scope: `${label} 单房经济/寿命/冷恢复；不含战斗、多房或完整 M2B`,
    bot,
    ticks,
    split,
    runs: [],
  };
  try {
    summary.executable_sha256 = sha256(readFileSync(binary));
    summary.upstream = prepare(botRoot, join(out, 'modules'));
    const scenario = join(out, 'scenario.json');
    copyFileSync(join(selectedFixture, 'scenario.json'), scenario);
    const run = (name, channel, rounds, extra, totalTicks, startTime, economy) => {
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
      const result = evaluateRun(report, { ticks: rounds, totalTicks, startTime, economy });
      result.checks.push(
        { name: '实际投影通道', passed: report.channel === channel, actual: report.channel },
        { name: 'CLI 成功退出', passed: exitCode === 0, actual: exitCode },
        {
          name: 'CLI 二进制身份一致',
          passed: report.executable_sha256 === summary.executable_sha256,
          actual: report.executable_sha256,
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
        true,
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
        true,
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
      '--bot': 'bot',
      '--ticks': 'ticks',
      '--split': 'split',
    };
    const seen = new Set();
    for (let i = 2; i < process.argv.length; i += 2) {
      const key = names[process.argv[i]],
        value = process.argv[i + 1];
      if (!key || seen.has(key) || !value || value.startsWith('--'))
        throw new Error('参数须为 --out/--bin/--bot-root/--bot/--ticks/--split 的唯一键值对');
      seen.add(key);
      options[key] =
        key === 'bot' ? value : key === 'ticks' || key === 'split' ? Number(value) : resolve(value);
    }
    if (!options.out || !isAbsolute(options.out))
      throw new Error('须用 --out 指定不存在的输出目录（父目录须已存在）');
    const summary = runAcceptance(options);
    console.log(`报告：${join(options.out, 'summary.json')}`);
    process.exitCode = summary.success ? 0 : 2;
    if (summary.error) console.error(summary.error);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
