#!/usr/bin/env node
// 夹具生成：kkscreeps export-fixtures（kk-cli 只读子命令）→ 逐夹具 differ
// 转换 → packages/renderer/fixtures/<名>.json 入库。
//
// 用法：node scripts/build-fixtures.mjs [--ticks N] [--binary 路径]
// 每次运行都先 cargo build -p kk-cli（增量构建秒级）——防过期二进制静默
// 产出陈旧语义夹具（评审 B 建议 4）；--binary 显式指定时跳过构建。
// 确定性链：固定 seed + 固定 TickInput（exporter）+ 纯函数 differ → 夹具
// 重生成逐字节一致（评审可复算）。
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = join(pkgRoot, '..', '..');
const fixturesDir = join(pkgRoot, 'fixtures');

const args = process.argv.slice(2);
const argOf = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const ticks = argOf('--ticks') ?? '30';
const explicitBinary = argOf('--binary');
const binary = explicitBinary ?? join(repoRoot, 'target', 'debug', 'kkscreeps');

if (!explicitBinary) {
  console.log('cargo build -p kk-cli（增量）…');
  const build = spawnSync('cargo', ['build', '-p', 'kk-cli'], { cwd: repoRoot, stdio: 'inherit' });
  if (build.status !== 0) process.exit(build.status ?? 1);
}

const { convertWorldJsonTicks } = await import(join(pkgRoot, 'src', 'feed', 'differ.ts'));

const outDir = mkdtempSync(join(tmpdir(), 'kk-fixtures-'));
try {
  const run = spawnSync(binary, ['export-fixtures', '--out', outDir, '--ticks', String(ticks)], {
    encoding: 'utf8',
  });
  if (run.error) {
    console.error('exporter 启动失败:', run.error);
    process.exit(1);
  }
  if (run.status !== 0) {
    console.error(run.stdout ?? '', run.stderr ?? '');
    process.exit(run.status ?? 1);
  }
  console.log(run.stdout.trim());

  mkdirSync(fixturesDir, { recursive: true });
  for (const file of readdirSync(outDir)
    .filter((f) => f.endsWith('.jsonl'))
    .sort()) {
    const ticksOut = readFileSync(join(outDir, file), 'utf8')
      .split('\n')
      .filter((l) => l.trim().length > 0)
      .map((l) => JSON.parse(l));
    const fixture = convertWorldJsonTicks(ticksOut);
    const outPath = join(fixturesDir, file.replace(/\.jsonl$/, '.json'));
    writeFileSync(outPath, JSON.stringify(fixture));
    const deltas = fixture.ticks.length - 1;
    const changed =
      deltas === 0
        ? 0
        : fixture.ticks.slice(1).filter((d) => d.upsert.length + d.removedIds.length > 0).length;
    console.log(`✓ ${outPath}（1 快照 + ${deltas} 增量，其中 ${changed} 个非空）`);
  }
} finally {
  rmSync(outDir, { recursive: true, force: true });
}
