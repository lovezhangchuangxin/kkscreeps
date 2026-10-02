#!/usr/bin/env node
// 提交信息校验（commit-msg hook）：subject 需符合「前缀：描述」格式。
// 全角、半角冒号均可；前缀和描述须非空，允许换行添加正文。
// 用法：node scripts/check-commit-msg.mjs <commit-msg 文件>（hook 以 $1 传入）

import { readFileSync } from 'node:fs';

// 长度均按 Unicode 码点计数，不是 UTF-8 字节数或终端显示宽度。
const MAX_SUBJECT_LENGTH = 350;
const MAX_PREFIX_LENGTH = 48;

// 匹配这些前缀的 merge/revert/fixup/squash 标题免检；这里只匹配文本，不验证提交类型。
const EXEMPT_RE = /^(Merge (?:branch(?:es)?|remote-tracking branch|pull request|tag|commit) |Revert |(?:fixup|squash)! )/;

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
  console.error(`  期望「前缀：描述」（冒号前 ≤${MAX_PREFIX_LENGTH} 字符），示例：B2 收口：灰区表回写（8 条）`);
  console.error('  确需跳过：git commit --no-verify（不推荐）');
  process.exit(1);
}

if (EXEMPT_RE.test(subject)) process.exit(0);
if (subject.trim().length === 0) reject('subject 不能为空');
if (/^\s|\s$/.test(subject)) reject('subject 首尾不能有空白字符（注意不可见的 CR/BOM 已归一，此处多为普通空格）');
if (chars.length > MAX_SUBJECT_LENGTH) {
  reject(`subject ${chars.length} 字符，超过上限 ${MAX_SUBJECT_LENGTH}`);
}

const m = /^([^：:]+)[：:](.*)$/.exec(subject);
if (!m) reject('subject 需符合「前缀：描述」格式——找不到冒号分隔（全角/半角冒号均可）');
const prefixLength = Array.from(m[1]).length;
if (prefixLength > MAX_PREFIX_LENGTH) {
  reject(`冒号前 ${prefixLength} 字符，超过上限 ${MAX_PREFIX_LENGTH}——前缀应是简短的轨道/批次/事项代号`);
}
if (m[2].trim().length === 0) reject('冒号后的描述不能为空');
