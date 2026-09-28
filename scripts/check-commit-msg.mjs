#!/usr/bin/env node
// 提交信息校验（commit-msg hook）：subject 需符合「前缀：描述」格式。
// 格式取自仓库既有惯例（如 `B2 收口：…`、`合入轨 C2/C2.1：…`），非 Conventional Commits。
// 用法：node scripts/check-commit-msg.mjs <commit-msg 文件>（hook 以 $1 传入）

import { readFileSync } from 'node:fs';

// 标定自全量历史回放（61 条 subject，码点计数）：最长 162、最长前缀 42。
const MAX_SUBJECT_LENGTH = 350; // 远超历史最长，只拦粘贴事故级超长
const MAX_PREFIX_LENGTH = 48; // 冒号前的「轨道/批次/事项前缀」长度上限，覆盖历史最长 42 留裕量

// git 自动生成或改写类提交不适用人工格式，直接放行（含章鱼合并与 git merge <sha>）。
const EXEMPT_RE = /^(Merge (?:branch(?:es)?|remote-tracking branch|pull request|tag|commit) |Revert |(?:fixup|squash)! )/;

const file = process.argv[2];
if (!file) {
  console.error('用法：node scripts/check-commit-msg.mjs <commit-msg-file>');
  process.exit(1);
}

let raw;
try {
  // 剥 BOM、归一 CRLF，避免不可见字符触发难自查的报错。
  raw = readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
} catch {
  console.error(`✗ 提交信息校验失败：无法读取 ${file}`);
  process.exit(1);
}

const subject = raw.split(/\r?\n/)[0];
const chars = Array.from(subject); // 码点计数，与中文/emoji 的视觉长度一致

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
