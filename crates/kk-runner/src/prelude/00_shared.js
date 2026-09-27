/* kk-runner 桩环境 prelude · 段 0：共享基础设施。
 *
 * 执行顺序（由 sandbox.rs 保证）：
 *   lodash 资产脚本 → 常量平铺脚本 → prelude 00_shared → 10_buffer → 20_cjs
 *   → 30_memory_world → 90_host（末段遮蔽 globalThis.Deno 并删除 __kkp）。
 *
 * 各段独立 IIFE、零全局词法绑定；段间共享只经 globalThis.__kkp（私有上下文，
 * 全部 prelude 段执行完由 90_host 删除，玩家代码不可见）。
 *
 * 本段：缺失 API 探测仪表、console、lodash 装配。
 * 桩环境（stub Game）服务于 bot 环境兼容性验收与 screeps-ok 适配器；
 * M2 起被真正的玩家 API 层（共享内存直读 + 惰性加载）取代。
 */
(() => {
  'use strict';

  const ops = globalThis.Deno && globalThis.Deno.core ? globalThis.Deno.core.ops : null;
  if (!ops || !ops.op_kk_module_kind) {
    throw new Error('kk-runner prelude: op 不可达（extension 未注册？）');
  }

  // ---------------------------------------------------------------- 缺失 API 探测
  // M0 环境验收的核心仪表：记录"玩家代码访问了但桩环境没有"的路径（含次数）。
  // get trap 覆盖属性访问/解构（GetV），has trap 覆盖 `in` 检查——`hasOwnProperty`/
  // Object.keys 枚举不触发 trap，属仪表的已知边界（枚举无法反推玩家想找什么）。

  const missing = new Map();
  function reportMissing(path) {
    missing.set(path, (missing.get(path) || 0) + 1);
  }
  // has-trap 的缺失记录独立成桶：`!('x' in obj)` 式否定能力探测与真实缺失在
  // trap 内不可区分，全部降为"低置信 has 探测"，不计入缺失清单（验收面只看
  // get-trap——访问了且拿不到才是确定的缺失）
  const hasProbes = new Map();
  // 'pos'：官方 RoomPosition 无 pos 属性（RoomObject 才有）——bot 的防御性访问
  // 在官方也得 undefined，不按缺失计；wrapObject 的对象自带真 pos 不受影响
  const PROBE_IGNORE = new Set(['then', 'constructor', '__proto__', 'prototype', 'inspect', 'toJSON', 'nodeType', 'length', 'name', 'pos']);
  function probe(target, basePath) {
    return new Proxy(target, {
      get(t, prop) {
        if (typeof prop === 'string' && !(prop in t) && !PROBE_IGNORE.has(prop)) {
          reportMissing(basePath + '.' + prop);
        }
        return t[prop];
      },
      has(t, prop) {
        // 含 ./[ 的"属性名"是 lodash 等库的路径探测串（如 isKey 对 'owner.username'
        // 整串做 in 检查），不是真实属性访问，不报仪表
        if (typeof prop === 'string' && !prop.includes('.') && !prop.includes('[')
          && !(prop in t) && !PROBE_IGNORE.has(prop)) {
          hasProbes.set(basePath + '.' + prop, (hasProbes.get(basePath + '.' + prop) || 0) + 1);
        }
        return prop in t;
      },
    });
  }

  // 桩方法调用计数：存在但不做事的官方方法（返回 OK），统计实际被用到的部分。
  const stubCalls = new Map();
  function countStub(name) {
    stubCalls.set(name, (stubCalls.get(name) || 0) + 1);
  }

  // ---------------------------------------------------------------- console
  // 收集型 console：经 op 推给宿主（替代 deno_core 自带的直写 stdout 版本）。
  // 套探测仪表：console.dir/trace 等官方缺口以 'console.*' 登记，不静默 undefined。

  // 官方格式化（engine/src/game/console.js log 分支逐参映射）：
  //   真值且有 toString → toString()（普通对象得 '[object Object]'，游戏对象得
  //   官方 toString 家族如 '[creep x]'，数组得逗号拼接）；
  //   undefined → 'undefined'；其余 falsy（null/0/false/''/NaN）→ JSON.stringify
  //   （null→'null'、NaN→'null'）。join(' ')。
  function fmtArgs(args) {
    return Array.prototype.map.call(args, (i) => {
      if (i && i.toString) return i.toString();
      if (typeof i === 'undefined') return 'undefined';
      return JSON.stringify(i);
    }).join(' ');
  }
  const mkLevel = (level) =>
    function (...args) {
      ops.op_kk_console(level, fmtArgs(args));
    };
  globalThis.console = probe({
    log: mkLevel('log'),
    info: mkLevel('info'),
    warn: mkLevel('warn'),
    error: mkLevel('error'),
    debug: mkLevel('debug'),
  }, 'console');

  // ---------------------------------------------------------------- lodash
  // lodash 资产脚本已执行并把结果放在 globalThis.__kkLodash（CJS 导出或 UMD 根挂载）。
  const _ = globalThis.__kkLodash;
  delete globalThis.__kkLodash;
  if (typeof _ !== 'function' || typeof _.map !== 'function') {
    throw new Error('kk-runner prelude: lodash 预置失败');
  }
  globalThis._ = _;

  // ---------------------------------------------------------------- 段间私有上下文
  // 90_host 执行完删除本属性；各段在自身加载时捕获所需引用进闭包。
  globalThis.__kkp = { ops, probe, reportMissing, PROBE_IGNORE, missing, hasProbes, stubCalls, countStub, lodash: _ };
})();
