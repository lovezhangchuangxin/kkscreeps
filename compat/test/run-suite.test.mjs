// 在临时目录运行真实脚本，用假构建命令和套件验证输入更新检查，无需跑完整套件。
import assert from 'node:assert/strict';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

const script = resolve(import.meta.dirname, '../scripts/run-suite.sh');

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'kk-run-suite-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const dir of [
    'compat/scripts',
    'compat/src',
    'compat/dist',
    '.compat/screeps-ok/node_modules',
    'bin',
  ]) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  copyFileSync(script, join(root, 'compat/scripts/run-suite.sh'));
  const sourceParity = join(root, 'compat/src/parity.json');
  const builtParity = join(root, 'compat/dist/parity.json');
  writeFileSync(sourceParity, '{"skips":{"new":true}}');
  writeFileSync(builtParity, '{"skips":{}}');
  writeFileSync(join(root, 'compat/src/adapter.ts'), '// source');
  writeFileSync(join(root, 'compat/dist/adapter.js'), '// built');
  const old = new Date('2020-01-01');
  const built = new Date('2021-01-01');
  utimesSync(sourceParity, old, old);
  utimesSync(join(root, 'compat/src/adapter.ts'), old, old);
  utimesSync(builtParity, built, built);
  utimesSync(join(root, 'compat/dist/adapter.js'), built, built);
  writeFileSync(
    join(root, 'bin/pnpm'),
    '#!/bin/sh\ntouch build-called\n[ "${FAIL_BUILD:-}" != 1 ] || exit 1\ncp src/parity.json dist/parity.json\n',
    { mode: 0o755 },
  );
  writeFileSync(join(root, 'bin/npx'), '#!/bin/sh\ntouch suite-called\n', { mode: 0o755 });
  return {
    root,
    sourceParity,
    builtParity,
    run: (extra = {}) =>
      spawnSync('bash', [join(root, 'compat/scripts/run-suite.sh')], {
        encoding: 'utf8',
        env: {
          ...process.env,
          PATH: join(root, 'bin') + ':' + process.env.PATH,
          KKSCREEPS_BIN: '/unused/fake-engine',
          ...extra,
        },
      }),
    built: () => existsSync(join(root, 'compat/build-called')),
    ran: () => existsSync(join(root, '.compat/screeps-ok/suite-called')),
  };
}

test('parity.json 更新后先重建再运行套件', (t) => {
  const f = fixture(t);
  const newer = new Date('2022-01-01');
  utimesSync(f.sourceParity, newer, newer);
  const run = f.run();
  assert.equal(run.status, 0, run.stderr);
  assert.ok(f.built(), '仅 JSON 更新也必须重建');
  assert.equal(readFileSync(f.builtParity, 'utf8'), readFileSync(f.sourceParity, 'utf8'));
  assert.ok(f.ran());
});

test('缺少构建后的 parity.json 时重建', (t) => {
  const f = fixture(t);
  rmSync(f.builtParity);
  const run = f.run();
  assert.equal(run.status, 0, run.stderr);
  assert.ok(f.built());
  assert.ok(existsSync(f.builtParity));
});

test('输入未更新时直接运行；TypeScript 更新仍触发重建', (t) => {
  const f = fixture(t);
  assert.equal(f.run().status, 0);
  assert.equal(f.built(), false);
  const newer = new Date('2022-01-01');
  utimesSync(join(f.root, 'compat/src/adapter.ts'), newer, newer);
  assert.equal(f.run().status, 0);
  assert.ok(f.built());
});

test('parity.json 更新但重建失败时不运行套件', (t) => {
  const f = fixture(t);
  rmSync(f.builtParity);
  const run = f.run({ FAIL_BUILD: '1' });
  assert.equal(run.status, 1);
  assert.equal(f.ran(), false);
});
