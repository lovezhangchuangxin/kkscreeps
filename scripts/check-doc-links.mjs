#!/usr/bin/env node
// 文档链接检查（M0 CI 条目）：校验仓库内 Markdown 的相对链接与锚点。
// 只做本地文件检查，不访问网络。用法：node scripts/check-doc-links.mjs

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

// GitHub 风格的标题锚点：小写、空格转连字符、去掉标点。
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

// [text](target) 形式；跳过 ![image](…) 也一并检查（图片链接同样要求存在）。
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
