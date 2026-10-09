// 原版 Overmind 发布资产身份与只读验收入口，模拟矩阵由 bot-gameplay 共用。
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

export const OVERMIND_VERSION = 'v0.5.2.1';
export const OVERMIND_SHA256 = 'e78ad3c8e58a955163fc81adf6be1d584c881081a6b0a78156d519f202b5a4b8';
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const fixture = resolve(import.meta.dirname, '../fixtures/sim-overmind');

export function prepareOvermindModules(botRoot, destination) {
  const actual = readdirSync(botRoot)
    .filter((name) => name.endsWith('.js'))
    .sort();
  if (JSON.stringify(actual) !== JSON.stringify(['main.js']))
    throw new Error('Overmind 发布目录须只有 main.js 模块');
  const bytes = readFileSync(join(botRoot, 'main.js'));
  if (sha256(bytes) !== OVERMIND_SHA256) throw new Error('Overmind 源码与锁定官方发布资产不符');
  mkdirSync(destination);
  writeFileSync(join(destination, 'main.js'), bytes);
  copyFileSync(join(fixture, 'acceptance.js'), join(destination, 'acceptance.js'));
  return {
    version: OVERMIND_VERSION,
    source: resolve(botRoot),
    release_url: `https://github.com/bencbartlett/Overmind/releases/download/${OVERMIND_VERSION}/main.js`,
    files: [{ name: 'main.js', sha256: OVERMIND_SHA256 }],
    observer_sha256: sha256(readFileSync(join(fixture, 'acceptance.js'))),
  };
}
