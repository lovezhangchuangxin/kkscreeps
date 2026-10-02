#!/usr/bin/env node
// 检查仓库 Markdown 的内联本地链接：目标文件存在，标题锚点可匹配。
// 不访问网络，也不解析完整 Markdown；引用式链接、围栏代码和重复标题的编号均未专门处理。
// 用法：node scripts/check-doc-links.mjs。

import { readdirSync, readFileSync, statSync, existsSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SKIP_DIRS = new Set(['.git', '.compat', 'node_modules', 'target', 'fixtures', '.zcode', 'spikes']);

function collectMarkdown(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) out.push(...collectMarkdown(full));
    else if (entry.endsWith('.md')) out.push(full);
  }
  return out;
}

// 简化的标题锚点：转小写、去标点、合并空白；不等同于完整 GitHub Markdown 规则。
function headingSlug(heading) {
  return heading
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');
}

function anchorsOf(mdPath) {
  const anchors = new Set();
  for (const line of readFileSync(mdPath, 'utf8').split('\n')) {
    const m = /^(#{1,6})\s+(.*)$/.exec(line);
    if (m) anchors.add(headingSlug(m[2].replace(/`/g, '')));
  }
  return anchors;
}

const files = collectMarkdown(repoRoot);
const errors = [];
let checked = 0;

// 匹配 [text](target) 和 ![image](target)，不支持带空白或嵌套括号的目标地址。
const LINK_RE = /\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;

for (const file of files) {
  const text = readFileSync(file, 'utf8');
  for (const match of text.matchAll(LINK_RE)) {
    const target = match[2];
    if (/^(https?:|mailto:|\/\/)/.test(target)) continue; // 外部链接不查
    checked++;

    const [pathPart, anchor] = target.split('#');
    const baseDir = pathPart === '' ? dirname(file) : join(dirname(file), pathPart);

    if (pathPart !== '' && !existsSync(baseDir)) {
      errors.push(`${rel(file)}: 链接目标不存在 → ${target}`);
      continue;
    }
    if (anchor !== undefined && anchor !== '') {
      // 当前文件链接直接查本文件；非 .md 目标按目录中的 README.md 查锚点。
      const mdFile = pathPart === '' ? file : baseDir.endsWith('.md') ? baseDir : join(baseDir, 'README.md');
      if (!existsSync(mdFile) || !anchorsOf(mdFile).has(anchor)) {
        errors.push(`${rel(file)}: 锚点不存在 → ${target}`);
      }
    }
  }
}

function rel(p) {
  return p.slice(repoRoot.length + 1);
}

if (errors.length > 0) {
  console.error(`文档链接检查失败（${errors.length} 处）：`);
  for (const e of errors) console.error('  ' + e);
  process.exit(1);
}
console.log(`文档链接检查通过：${files.length} 个 Markdown，${checked} 个本地链接。`);
