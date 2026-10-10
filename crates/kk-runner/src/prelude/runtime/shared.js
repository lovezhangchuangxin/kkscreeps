/* 装配 console、lodash 和 API 访问诊断，建立各 prelude 段共用的临时 __kkp。
 * 所有段加载后，Rust 捕获宿主函数并删除 __kkp；运行时使用各段已捕获的闭包引用。
 * 诊断分别记录缺失属性访问与 in 探测，不能据此推断枚举操作的意图。
 */
(() => {
  'use strict';

  const ops = globalThis.Deno && globalThis.Deno.core ? globalThis.Deno.core.ops : null;
  if (!ops || !ops.op_kk_module_kind) {
    throw new Error('kk-runner prelude: op 不可达（extension 未注册？）');
  }

  // 捕获 map 与 Reflect.apply，避免 console 格式化随玩家替换这些方法而改变。
  const __reflectApply = Reflect.apply;
  const __arrayMap = Array.prototype.map;

  // 记录玩家访问的缺失属性与次数。get 和 in 会触发诊断，Object.keys/hasOwnProperty 不会。

  const missing = new Map();
  function reportMissing(path) {
    missing.set(path, (missing.get(path) || 0) + 1);
  }
  // in 检查可能只是能力探测，单独计入 hasProbes，不计入缺失属性清单。
  const hasProbes = new Map();
  // 忽略 Promise 检查、序列化和内部坐标探测等常见属性访问，降低诊断噪声。
  // 这些豁免只影响诊断；不存在的属性仍返回 undefined。
  const PROBE_IGNORE = new Set([
    'then',
    'constructor',
    '__proto__',
    'prototype',
    'inspect',
    'toJSON',
    'nodeType',
    'length',
    'name',
    'pos',
    '__packedPos',
    'x',
    'y',
  ]);
  // 仅豁免 Flag.ref、StructureSpawn.storeCapacity 与 Room 挂载三键的缺失诊断。
  // Room.controller/storage/terminal 官方为「结构存在才有键」（structures.js:41-56
  // 构造器挂载）——建造前读 undefined 属官方语义，逐 tick 的建造前探测不是
  // 缺 API。按标签冒号前的类名判断；旗帜名中包含冒号不影响分类。
  // 来源：上游 engine/src/game/flags.js 与 structures.js 的属性定义。
  const PROBE_IGNORE_SCOPED = new Set([
    'Flag.ref',
    'StructureSpawn.storeCapacity',
    'Room.controller',
    'Room.storage',
    'Room.terminal',
  ]);
  const classOfLabel = (basePath) => {
    // 两段剥离：冒号前（命名对象如 'Creep:采集者'）与方括号前（带名标签
    // 如 'Room[W1N1]'）——都取类名部分做豁免匹配。
    let s = basePath;
    const i = s.indexOf(':');
    if (i !== -1) s = s.slice(0, i);
    const b = s.indexOf('[');
    if (b !== -1) s = s.slice(0, b);
    return s;
  };
  // 宿主在沙箱创建时决定是否装配诊断仪表（SandboxConfig.api_probe）。
  // 关闭时原样返回目标：字段访问直达内联缓存，省去两层 Proxy 拦截
  //（实测占 run 相约四分之一墙钟）。missing/hasProbes 报告为空是
  // 关闭时的预期形态，bot-check 等诊断工作流保持默认开启。
  const PROBE_ENABLED = ops.op_kk_probe_enabled();
  function probe(target, basePath) {
    if (!PROBE_ENABLED) return target;
    return new Proxy(target, {
      get(t, prop) {
        if (
          typeof prop === 'string' &&
          !(prop in t) &&
          !PROBE_IGNORE.has(prop) &&
          !PROBE_IGNORE_SCOPED.has(classOfLabel(basePath) + '.' + prop)
        ) {
          reportMissing(basePath + '.' + prop);
        }
        return t[prop];
      },
      has(t, prop) {
        // 含 ./[ 的"属性名"是 lodash 等库的路径探测串（如 isKey 对 'owner.username'
        // 整串做 in 检查），不是真实属性访问，不报仪表
        if (
          typeof prop === 'string' &&
          !prop.includes('.') &&
          !prop.includes('[') &&
          !(prop in t) &&
          !PROBE_IGNORE.has(prop)
        ) {
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
  // falsy 分支官方用调用时全局 stringify（console.js:25 逐字）——玩家覆写全局
  // 影响自家 console 渲染是官方行为，保持一致
  function fmtArgs(args) {
    return __reflectApply(__arrayMap, args, [
      (i) => {
        if (i && i.toString) return i.toString();
        if (typeof i === 'undefined') return 'undefined';
        return JSON.stringify(i);
      },
    ]).join(' ');
  }
  const mkLevel = (level) =>
    function (...args) {
      ops.op_kk_console(level, fmtArgs(args));
    };
  globalThis.console = probe(
    {
      log: mkLevel('log'),
      info: mkLevel('info'),
      warn: mkLevel('warn'),
      error: mkLevel('error'),
      debug: mkLevel('debug'),
    },
    'console',
  );

  // ---------------------------------------------------------------- lodash
  // lodash 资产脚本已执行并把结果放在 globalThis.__kkLodash（CJS 导出或 UMD 根挂载）。
  const _ = globalThis.__kkLodash;
  delete globalThis.__kkLodash;
  if (typeof _ !== 'function' || typeof _.map !== 'function') {
    throw new Error('kk-runner prelude: lodash 预置失败');
  }
  globalThis._ = _;

  // ---------------------------------------------------------------- 段间私有上下文
  // Rust 捕获宿主句柄后删除本属性；各段在自身加载时捕获所需引用进闭包。
  globalThis.__kkp = {
    ops,
    probe,
    reportMissing,
    PROBE_IGNORE,
    missing,
    hasProbes,
    stubCalls,
    countStub,
    lodash: _,
    // 通过闭包中的 op 请求主动中断；Rust 据此区分 halt 与预算超时。
    cpuHalt: () => ops.op_kk_cpu_halt(),
  };
})();
