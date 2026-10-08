#!/usr/bin/env node
// 提交信息校验（commit-msg hook）：subject 需符合 Conventional Commits 格式。
// 首行形如 `type(scope): 描述`；scope 与破坏性标记 `!` 可选，冒号须为
// 半角且后接一个空格。type 词表固定，scope 允许字母/数字/`._-`。
// 用法：node scripts/check-commit-msg.mjs <commit-msg 文件>（hook 以 $1 传入）

import { readFileSync } from 'node:fs';

// 长度均按 Unicode 码点计数，不是 UTF-8 字节数或终端显示宽度。
const MAX_SUBJECT_LENGTH = 350;
const MAX_PREFIX_LENGTH = 48;

const TYPES = [
  'feat',
  'fix',
  'docs',
  'style',
  'refactor',
  'perf',
  'test',
  'build',
  'ci',
  'chore',
  'revert',
];
// type(scope)!?: 空格 描述。scope 段独立成组便于报错时点名。
const SUBJECT_RE = new RegExp(`^(${TYPES.join('|')})(\\([A-Za-z0-9_.-]+\\))?!?: (.+)$`);

// 匹配这些前缀的 merge/revert/fixup/squash 标题免检；这里只匹配文本，
// 不验证提交类型（小写 `revert:` 类型本身在词表内，走常规校验）。
const EXEMPT_RE =
  /^(Merge (?:branch(?:es)?|remote-tracking branch|pull request|tag|commit) |Revert |(?:fixup|squash)! )/;

const file = process.argv[2];
if (!file) {
  console.error('用法：node scripts/check-commit-msg.mjs <commit-msg-file>');
  process.exit(1);
}

let raw;
try {
  // 去除文件开头的 BOM；下方按 LF 或 CRLF 分割，仅读取首行标题。
  raw = readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
} catch {
  console.error(`✗ 提交信息校验失败：无法读取 ${file}`);
  process.exit(1);
}

const subject = raw.split(/\r?\n/)[0];
const chars = Array.from(subject); // 组合字符或多个码点组成的 emoji 会计为多个。

function reject(reason) {
  console.error(`✗ 提交信息校验失败：${reason}`);
  const preview = chars.slice(0, 80).join('');
  console.error(`  subject：${preview}${chars.length > 80 ? '…' : ''}`);
  console.error(`  期望 Conventional Commits：type(scope)!: 描述，type ∈ {${TYPES.join(', ')}}`);
  console.error('  示例：fix(kk-schema): 解码器拒绝零行桶');
  console.error('  确需跳过：git commit --no-verify（不推荐）');
  process.exit(1);
}

if (EXEMPT_RE.test(subject)) process.exit(0);
if (subject.trim().length === 0) reject('subject 不能为空');
if (/^\s|\s$/.test(subject))
  reject('subject 首尾不能有空白字符（注意不可见的 CR/BOM 已归一，此处多为普通空格）');
if (chars.length > MAX_SUBJECT_LENGTH) {
  reject(`subject ${chars.length} 字符，超过上限 ${MAX_SUBJECT_LENGTH}`);
}

const colonAt = subject.indexOf(':');
const m = colonAt === -1 ? null : SUBJECT_RE.exec(subject);
if (!m) {
  // 分类报错，避免所有非法输入都拿到同一条含糊信息。
  if (colonAt === -1) reject('找不到半角冒号——subject 需符合 type(scope)!: 描述');
  const beforeColon = subject.slice(0, colonAt);
  const typeOnly = /^([A-Za-z]+)(\([A-Za-z0-9_.-]+\))?!?$/.exec(beforeColon);
  if (typeOnly && !TYPES.includes(typeOnly[1])) {
    reject(`未知 type「${typeOnly[1]}」——词表固定为 {${TYPES.join(', ')}}`);
  }
  if (
    /^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([A-Za-z0-9_.-]+\))?!?:/.test(
      subject,
    ) &&
    !/^ /.test(subject.slice(colonAt + 1))
  ) {
    reject('冒号后须紧跟一个空格：`type: 描述`');
  }
  if (/[：]/.test(beforeColon)) {
    reject('检测到全角冒号——Conventional Commits 使用半角冒号');
  }
  reject('subject 需符合 type(scope)!: 描述——scope 与 ! 可选，冒号后须有一个空格');
}
const prefixLength = Array.from(m[1] + (m[2] ?? '')).length;
if (prefixLength > MAX_PREFIX_LENGTH) {
  reject(`type 与 scope 合计 ${prefixLength} 字符，超过上限 ${MAX_PREFIX_LENGTH}`);
}
