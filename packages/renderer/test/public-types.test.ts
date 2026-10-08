import { describe, it, expect } from 'vitest';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';

const guard = resolve('scripts/check-public-types.mjs');
function check(files: Record<string, string>) {
  const dir = mkdtempSync(join(tmpdir(), 'r7-dts-'));
  try {
    for (const [path, text] of Object.entries(files)) {
      const file = join(dir, path);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, text);
    }
    return spawnSync(process.execPath, [guard, join(dir, 'index.d.ts')], { encoding: 'utf8' });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
describe('public declaration graph (real TS native compiler)', () => {
  it.each([
    "export type Bad = import('pixi.js').Texture;",
    'import type { Texture } from "pixi.js/lib/index.js"; export type Bad = Texture;',
    "export * from '@pixi/core';",
  ])('rejects dynamic and Pixi subpath types: %s', (index) => {
    const result = check({ 'index.d.ts': index });
    expect(result.status).toBe(1);
    expect(result.stderr).toContain('Pixi public type');
  });
  it('follows bare type packages through their types entry', () => {
    const r = check({
      'index.d.ts': 'export * from "type-door";',
      'node_modules/type-door/package.json': '{"name":"type-door","types":"index.d.ts"}',
      'node_modules/type-door/index.d.ts': 'export type Bad = import("pixi.js").Texture;',
    });
    expect(r.status).toBe(1);
    expect(r.stderr).toContain('Pixi public type');
  });
  it('follows relative dynamic types and nested double quoted exports', () => {
    const r = check({
      'index.d.ts': 'export type Bad = import("./nested/a.js").Bad;',
      'nested/a.d.ts': 'export { Bad } from "../b.js";',
      'b.d.ts': 'export type Bad = import("pixi.js").Texture;',
    });
    expect(r.status).toBe(1);
    expect(r.stderr).toContain('Pixi public type');
  });
  it('allows cyclic ordinary data and unreachable internal Pixi declarations', () => {
    const r = check({
      'index.d.ts': 'export { A } from "./nested/a.js";',
      'nested/a.d.ts': 'import type { B } from "../b.js"; export interface A { b: B }',
      'b.d.ts': 'import type { A } from "./nested/a.js"; export interface B { a: A }',
      'internal.d.ts': 'export type Private = import("pixi.js").Texture;',
    });
    expect(r.status).toBe(0);
    expect(r.stdout).toContain('3 文件');
  });
  it('rejects unresolved declarations rather than silently ending the graph', () => {
    const r = check({ 'index.d.ts': 'export * from "absent-door";' });
    expect(r.status).toBe(1);
    expect(r.stderr).toContain('Unresolved');
  });
  it('does not treat comments or string literal data as module imports', () => {
    const r = check({
      'index.d.ts': '// import("pixi.js").Texture\nexport type Label = "pixi.js";',
    });
    expect(r.status).toBe(0);
  });
});
