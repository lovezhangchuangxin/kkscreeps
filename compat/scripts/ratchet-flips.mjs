// 棘轮逐用例回归的判定核心（纯函数，ratchet.mjs 与单测共用）。
// 独立成模块的原因：对冲场景（一条转跳过 + 一条转通过保住总数）只有
// 单测能锁死——门禁脚本自身不可 import（顶层副作用会跑全量套件）。

/** 报告 → fullName 到 assertion 记录的映射（vitest json reporter 形状）。 */
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
 * 转红：基线 passed 的用例在当前**非 passed**——缺失、失败或转跳过同罪。
 * passed→skipped 必须与 failed 同级：只认 failed 时，"一条转跳过 + 一条
 * 转通过"可同时保住 passed 总数与用例总数，三重门禁全数放行（对冲漏洞）。
 * 转绿：基线非 passed → 当前 passed（不设限，floor 兜总量）。
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
