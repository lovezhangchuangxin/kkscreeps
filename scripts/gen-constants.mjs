#!/usr/bin/env node
// 从官方常量 dump（crates/kk-runner/assets/constants.json，产自官方
// common/lib/constants.js 的全量求值）生成 kk-core 的 Rust 常量表。
//
// 用法：node scripts/gen-constants.mjs
// 产物：crates/kk-core/src/constants.rs（提交入库；改上游 dump 后重跑本脚本）。
//
// 类型映射（按值的实际形态分类，生成器不做语义假设）：
//   int            → pub const X: i64
//   float          → pub const X: f64
//   str            → pub const X: &str
//   [int]          → pub const X: &[i64]
//   [str]          → pub const X: &[&str]
//   {k: 数}         → pub const X: &[(&str, i64 | f64)]
//   {k: str}       → pub const X: &[(&str, &str)]
//   {k: [int]}     → pub const X: &[(&str, &[i64])]
//   其他（嵌套对象）→ pub const X: &str = r#"…"#（内嵌 JSON，消费侧按需解析）

import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const input = join(root, 'crates/kk-runner/assets/constants.json');
const output = join(root, 'crates/kk-core/src/constants.rs');

const data = JSON.parse(readFileSync(input, 'utf8'));

const isInt = (v) => typeof v === 'number' && Number.isInteger(v) && !Object.is(v, -0);
const isFloat = (v) => typeof v === 'number' && !Number.isInteger(v);
const isStr = (v) => typeof v === 'string';
const listOf = (arr, pred) => Array.isArray(arr) && arr.every(pred);

function classify(v) {
  if (isInt(v)) return { kind: 'int', value: `${v}i64` };
  if (isFloat(v)) return { kind: 'float', value: floatLit(v) };
  if (isStr(v)) return { kind: 'str', value: JSON.stringify(v) };
  if (listOf(v, isInt)) return { kind: 'int_list', value: `&[${v.map((x) => `${x}`)}]` };
  if (listOf(v, isStr)) return { kind: 'str_list', value: `&[${v.map((x) => JSON.stringify(x))}]` };
  if (typeof v === 'object' && v !== null && !Array.isArray(v)) {
    const values = Object.values(v);
    if (values.length > 0 && values.every((w) => isInt(w) || isFloat(w))) {
      const allInt = values.every(isInt);
      const suffix = allInt ? 'i64' : '';
      return {
        kind: 'num_map',
        value: `&[${values.map((w, i) => `(${JSON.stringify(Object.keys(v)[i])}, ${isInt(w) && !allInt ? intAsFloat(w) : w + suffix})`)}]`,
        rustType: allInt ? '&[(&str, i64)]' : '&[(&str, f64)]',
      };
    }
    if (values.length > 0 && values.every(isStr)) {
      return {
        kind: 'str_map',
        value: `&[${values.map((w, i) => `(${JSON.stringify(Object.keys(v)[i])}, ${JSON.stringify(w)})`)}]`,
        rustType: '&[(&str, &str)]',
      };
    }
    if (values.length > 0 && values.every((w) => listOf(w, isInt))) {
      return {
        kind: 'int_list_map',
        value: `&[${values.map((w, i) => `(${JSON.stringify(Object.keys(v)[i])}, &[${w.map((x) => `${x}`)}])`)}]`,
        rustType: '&[(&str, &[i64])]',
      };
    }
  }
  // 嵌套对象（BOOSTS/POWER_INFO/REACTIONS/COMMODITIES 等）：内嵌 JSON 字符串
  return { kind: 'json', value: `r##"${JSON.stringify(v)}"##` };
}

function floatLit(v) {
  // 保证 f64 字面量带小数点/指数（1e-7 → 1e-7_f64 形态由后缀处理）
  let s = String(v);
  if (!s.includes('.') && !s.includes('e') && !s.includes('E')) s += '.0';
  return `${s}_f64`;
}
function intAsFloat(v) {
  let s = String(v);
  if (!s.includes('.') && !s.includes('e')) s += '.0';
  return `${s}_f64`;
}

function rustIdent(name) {
  // 常量名均为合法 SCREAMING_SNAKE；防御性处理数字开头
  return /^[0-9]/.test(name) ? `_${name}` : name;
}

let out = `\
//! 官方常量表（生成物，不手改）。
//!
//! 来源：\`crates/kk-runner/assets/constants.json\`（官方 \`common/lib/constants.js\`
//! 全量求值 dump）。重新生成：\`node scripts/gen-constants.mjs\`。
//! 键序保持官方文件顺序；嵌套对象（BOOSTS/POWER_INFO 等）以内嵌 JSON 字符串
//! 提供，消费侧按需解析。
#![allow(clippy::all)]

`;

for (const [key, value] of Object.entries(data)) {
  const c = classify(value);
  let type;
  switch (c.kind) {
    case 'int': type = 'i64'; break;
    case 'float': type = 'f64'; break;
    case 'str': type = '&str'; break;
    case 'int_list': type = '&[i64]'; break;
    case 'str_list': type = '&[&str]'; break;
    case 'json': type = '&str'; break;
    default: type = c.rustType;
  }
  out += `pub const ${rustIdent(key)}: ${type} = ${c.value};\n`;
}

// 先落盘再过 rustfmt：rustfmt 只就地格式化已存在的文件，落盘必须在其之前，
// 否则重生成不生效；rustfmt 失败时产物已写入但未格式化，退出码显形、本地
// 补跑后再提交（M3 的 CI 新鲜度校验依赖"重生成与入库文件零 diff"这一性质）
writeFileSync(output, out);
const fmt = spawnSync('rustfmt', ['--edition', '2024', output], { stdio: 'inherit' });
if (fmt.error || fmt.status !== 0) {
  console.error(`rustfmt 不可用或失败（status=${fmt.status ?? fmt.error.code}），已写入未格式化产物——本地补跑 rustfmt 后再提交`);
  process.exit(1);
}

// 自校验：重新读取生成物，确认每个源键都以 `pub const <KEY>:` 形态出现
const written = readFileSync(output, 'utf8');
const missing = Object.keys(data).filter((k) => !written.includes(`pub const ${/^[0-9]/.test(k) ? '_' : ''}${k}:`));
if (missing.length > 0) {
  console.error(`生成物缺键: ${missing.join(', ')}`);
  process.exit(1);
}
console.log(`生成 ${Object.keys(data).length} 个常量 → ${output}（rustfmt 已格式化，自校验通过）`);
