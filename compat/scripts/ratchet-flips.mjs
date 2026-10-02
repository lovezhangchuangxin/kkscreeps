// 比较两份报告的逐用例状态，并按豁免表分类回归。
// 单独导出纯函数，避免单元测试导入 ratchet.mjs 时启动完整套件。

/** 按 fullName 建索引，缺失时回退到 title；同名记录会被后项覆盖，报告应提供唯一名称。 */
export function statusesOf(report) {
  const out = new Map();
  for (const tr of report.testResults ?? []) {
    for (const a of tr.assertionResults ?? tr.assertions ?? []) {
      out.set(a.fullName ?? a.title ?? '', a);
    }
  }
  return out;
}

/**
 * 回归：基线通过的用例在当前失败、跳过或缺失。
 * 跳过也需计为回归，否则一条转跳过、另一条转通过可保持总通过数而隐藏覆盖损失。
 * 改善：当前通过、基线未通过或不存在；新增通过用例也计入。
 */
export function analyzeFlips(baselineReport, currentReport) {
  const baseline = statusesOf(baselineReport);
  const now = statusesOf(currentReport);
  const flipsRed = [];
  let flipsGreen = 0;
  for (const [name, a] of baseline) {
    if (a.status !== 'passed') continue;
    const cur = now.get(name);
    if (!cur || cur.status !== 'passed') flipsRed.push([name, cur]);
  }
  for (const [name, a] of now) {
    if (a.status === 'passed' && baseline.get(name)?.status !== 'passed') flipsGreen += 1;
  }
  return { flipsRed, flipsGreen };
}

/**
 * 按显式豁免表分类回归：
 * - 条目为用例 fullName 精确匹配，尾缀 `*` 为前缀匹配；
 * - blocked：未豁免的转红——门禁红；
 * - allowed：已豁免的回归，调用方打印用例名称；
 * - stale：未命中任何回归的条目，调用方仅警告；未命中不一定表示已恢复通过。
 */
export function applyAllow(flipsRed, allowPatterns) {
  const matches = (name, pattern) =>
    pattern.endsWith('*') ? name.startsWith(pattern.slice(0, -1)) : name === pattern;
  const blocked = [];
  const allowed = [];
  const hit = allowPatterns.map(() => false);
  for (const entry of flipsRed) {
    const name = entry[0];
    let exempt = false;
    allowPatterns.forEach((p, i) => {
      if (matches(name, p)) {
        hit[i] = true;
        exempt = true;
      }
    });
    if (exempt) allowed.push(entry);
    else blocked.push(entry);
  }
  const stale = allowPatterns.filter((_, i) => !hit[i]);
  return { blocked, allowed, stale };
}
