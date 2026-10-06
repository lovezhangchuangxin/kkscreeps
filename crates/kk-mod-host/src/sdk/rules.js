import { sealed, rulesInstalled, setRules } from './state.js';
import { anyKey } from './registration.js';

// 宿主在密封后随执行链安装冻结有效规则；只能调用一次，载荷来自 Rust 侧
// 冻结计划，JS 不接受注册代码改写或补充。重复安装、密封前调用与畸形
// 载荷都抛错，使安装失败沿注册失败密封 realm。
export function installRules(entries) {
  if (!sealed || rulesInstalled) throw new Error('有效规则只能在密封后安装一次');
  if (!Array.isArray(entries)) throw new Error('有效规则安装载荷须为数组');
  // 先验证完整载荷；畸形条目、重复键或 getter 抛错不能留下可读取的半张表。
  const nextRules = new Map();
  for (const entry of entries) {
    const key = entry && entry.key;
    const value = entry && entry.value;
    if (typeof key !== 'string' || typeof value !== 'number' || !Number.isFinite(value)) {
      throw new Error('有效规则条目须为 { key, value } 且值为有限数');
    }
    anyKey(key, '规则安装');
    if (nextRules.has(key)) throw new Error(`规则 ${key} 重复安装`);
    nextRules.set(key, value);
  }
  setRules(nextRules);
}

