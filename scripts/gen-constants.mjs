#!/usr/bin/env node
// 将 crates/kk-runner/assets/constants.json 转成 kk-core 的 Rust 常量表。
// 输入是上游 common/lib/constants.js 的求值结果；本脚本不读取或更新上游源码。
//
// 用法：node scripts/gen-constants.mjs
// 产物：crates/kk-core/src/constants.rs（提交入库；改上游 dump 后重跑本脚本）。
//
// 按 JSON 值的形态选择 Rust 类型；同一数值表含小数时，整数也转为 f64。
// 空数组按整数数组处理；空对象或无法匹配下列类型的值保存为 JSON 字符串。
// 类型映射：
//   int            → pub const X: i64
//   float          → pub const X: f64
//   str            → pub const X: &str
//   [int]          → pub const X: &[i64]
//   [str]          → pub const X: &[&str]
//   {k: 数}         → pub const X: &[(&str, i64 | f64)]
//   {k: str}       → pub const X: &[(&str, &str)]
//   {k: [int]}     → pub const X: &[(&str, &[i64])]
//   其他           → pub const X: &str = r##"…"##（调用方按需解析 JSON）

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
  // 无法匹配简单表类型时保留 JSON，避免丢失嵌套结构或混合类型。
  return { kind: 'json', value: `r##"${JSON.stringify(v)}"##` };
}

function floatLit(v) {
  // 使用小数点或指数表示浮点值，并用后缀明确指定 f64。
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
  // 数字开头时补下划线；其余名称直接使用，输入键需已满足 Rust 标识符规则。
  return /^[0-9]/.test(name) ? `_${name}` : name;
}

let out = `\
//! 由 JSON 常量数据生成的 Rust 常量表。
//!
//! 数据来源：\`crates/kk-runner/assets/constants.json\`，来自上游 \`common/lib/constants.js\`。
//! 修改数据或生成器后运行 \`node scripts/gen-constants.mjs\`，不要直接修改本文件。
//! 常量按输入对象的 JavaScript 枚举顺序生成；复杂值保存为 JSON 字符串，调用方按需解析。
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

// rustfmt 就地处理已写入的文件；失败时退出非零，但不会恢复旧产物。
writeFileSync(output, out);
const fmt = spawnSync('rustfmt', ['--edition', '2024', output], { stdio: 'inherit' });
if (fmt.error || fmt.status !== 0) {
  console.error(`rustfmt 不可用或失败（status=${fmt.status ?? fmt.error.code}），已写入未格式化产物——本地补跑 rustfmt 后再提交`);
  process.exit(1);
}

// 只检查每个源键是否出现在声明中；此检查不能证明生成值与 Rust 类型正确。
const written = readFileSync(output, 'utf8');
const missing = Object.keys(data).filter((k) => !written.includes(`pub const ${/^[0-9]/.test(k) ? '_' : ''}${k}:`));
if (missing.length > 0) {
  console.error(`生成物缺键: ${missing.join(', ')}`);
  process.exit(1);
}
console.log(`生成 ${Object.keys(data).length} 个常量 → ${output}（rustfmt 已格式化，自校验通过）`);
