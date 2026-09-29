/* kk-runner 桩环境 prelude · 段 4（末段）：宿主通道 + 遮蔽。
 *
 * 宿主 tick 通道（beginTick/runLoop/runExpr/endTick/resetModules/setRawMemory/
 * 仪表排干）注册进 `__kkp.host` 私有容器——**不上玩家全局**（B0 WP4 宿主面
 * 收口，ADR-013 决策 4）：Rust 侧在 prelude 装配完成后立即捕获私有
 * v8::Global 函数句柄，随后删除 __kkp。玩家全局零宿主通道——官方玩家侧
 * 本就没有宿主 tick 函数（runtime 在 driver 进程），原 `globalThis.__kk`
 * 冻结仍可调用的面（清自己指令/伪造世界注入/抢先排空仪表）就此消灭。
 * 本段末尾遮蔽 deno_core 内部面；__kkp 的删除由 Rust 在句柄捕获后执行。
 */
(() => {
  'use strict';

  // 预置 JSON 函数（官方 systemFunctions.JSON 同款语义）：宿主消息编码不得
  // 经玩家可覆写的全局——覆写全局 stringify 曾可伪造 intentsList（外部复审
  // P1 实证：无 API 调用造出工地）。玩家自己的代码仍用全局 JSON，不受影响。
  // 信封一律 null 原型 + 出口一律净化序列化（sanitizedStringify——序列化窗口
  // 内摘除根原型 toJSON，对抗评审 P1 的 Object/Array 绑定交换与数组 toJSON 面；
  // 官方 ExternalCopy 出口对玩家 toJSON 同样不可见，runtime.js:296）。
  // 回程 parse 预置化（自审 P2）；eval 捕获（对抗评审 P2：全局 eval 可交换使
  // 探针代码根本不执行）。
  const stringify = JSON.stringify;
  const parse = JSON.parse;
  const __eval = eval;
  const gw = globalThis.__kkp.gw;
  const nullEnv = gw.nullEnv;
  const safeStringify = gw.sanitizedStringify;
  const hostObject = gw.__Object;
  // 宿主信封 error 串的字节闸（A5-core 对抗评审 P3a，B0 补闸）：error 串拼
  // 玩家可控 message/stack（大小无闸），而 console 闸门不覆盖信封——出口处
  // 截断，防 exec_json 出口的跨边界放大（16K 码元诊断余量充足）
  const HOST_ERROR_MAX = 16 * 1024;
  function clipHostError(s) {
    return s.length > HOST_ERROR_MAX ? s.slice(0, HOST_ERROR_MAX) + '…[截断]' : s;
  }
  // 玩家异常对象的消息拼接安全化：e.message/stack 可是污染 getter（抛错逃逸
  // 会被宿主误判硬中断并错误计费）——不可打印错误降级为占位文本
  function safeErrorText(e, preferStack) {
    try {
      const m = preferStack ? (e && e.stack) || e : (e && e.message) || e;
      return clipHostError(String(m));
    } catch (_) {
      return 'unprintable player error';
    }
  }

  const { missing, hasProbes, stubCalls } = globalThis.__kkp;
  const { resetForNewModules } = globalThis.__kkp.cjs;
  const { beginTick, writeBackMemory, intentsPayload, setRawMemoryHost } = globalThis.__kkp.memoryWorld;

  globalThis.__kkp.host = {
    /** 每 tick 注入世界快照并重建 Game。cpuMeta：宿主预算真值（Rust 侧
     * begin_tick 第二参数——避免整世界深拷贝覆写三键，JS 侧合并覆盖占位基准）。 */
    beginTick(worldJson, cpuMeta) {
      beginTick(worldJson, cpuMeta);
    },
    /** 执行玩家入口：require('main').loop()。返回结果 JSON 字符串（宿主 exec_json 约定）。 */
    runLoop() {
      let out;
      try {
        const main = globalThis.require('main');
        if (main && typeof main.loop === 'function') {
          main.loop();
          out = nullEnv({ ok: true });
        } else {
          // 官方 engine/src/game/game.js:517-548 run()：`if(isObject && isFunction(loop)) {...}` 无
          // else——未导出 loop() 的 main 静默跳过，不是玩家错误（不打 console、
          // 不计入错误统计；bot-check 的 errored_ticks 口径据此）
          out = nullEnv({ ok: true });
        }
      } catch (e) {
        // 官方行为：未捕获的玩家错误打进 console（bot-check 据此采集每 tick 错误流）
        const msg = safeErrorText(e, true);
        console.error(msg);
        out = nullEnv({ ok: false, kind: 'runtime', error: msg });
      }
      try {
        return safeStringify(out);
      } catch (e) {
        // 字面量终极兜底（对抗评审 P1）：sanitizedStringify 自身失败（如根原型
        // 被 freeze）时不得二次抛——逃逸会被宿主判硬中断并误计费
        return '{"ok":true,"note":"loop 完成（结果不可序列化）"}';
      }
    },
    /** 求值一段玩家表达式代码，返回末表达式值（screeps-ok runPlayer 通道）。
     *  返回结果 JSON 字符串（宿主 exec_json 约定）。 */
    runExpr(code) {
      let out;
      let v;
      try {
        // 间接 eval（(0,eval)）在全局作用域求值——切断对 prelude 闭包（含捕获的
        // Deno.core.ops 引用等引擎内部状态）的访问；官方控制台命令同样在全局作用域
        // 求值（game.js:535-547），不得有超常特权（screeps-ok 契约）
        // eslint-disable-next-line no-eval
        v = __eval(code);
      } catch (e) {
        // eval 在全局作用域抛出的 SyntaxError 即解析期错误（官方控制台同口径）；
        // 运行期错误（含运行期动态构造的语法异常）归 runtime
        const kind = e instanceof SyntaxError ? 'syntax' : 'runtime';
        out = nullEnv({ ok: false, kind, error: safeErrorText(e, false) });
        return safeStringify(out);
      }
      if (v === undefined) {
        return safeStringify(nullEnv({ ok: true, value: null }));
      }
      try {
        // 活游戏对象检测，逐条对齐 vanilla 参照实现（buildConsoleCommand）：
        //  function/symbol → serialization；非数组对象且 constructor 不是
        //  Object/undefined（引擎类实例，如 RoomPosition/Creep/Room）→ serialization。
        // 检测代码自身抛错（如 constructor 为 null 的病态对象）不得逃逸——逃逸会被
        // 宿主误判为硬中断并错误计费；vanilla 的 wrapper 同样整体 catch
        let serErr = null;
        if (typeof v === 'function' || typeof v === 'symbol') {
          serErr = 'Return value is a ' + typeof v + ', not a plain JSON value';
        } else if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
          const ctor = v.constructor;
          if (ctor !== Object && ctor !== undefined) {
            // ctor.name 可是污染 getter 的任意长串——过闸（B0-3 评审 P2 清偿）
            const rawName = (ctor && typeof ctor === 'object' && ctor.name) || 'non-plain';
            const ctorName = clipHostError(String(rawName));
            serErr = 'Return value is a ' + ctorName + ' object, not a plain JSON value';
          }
        }
        if (serErr !== null) {
          return safeStringify(nullEnv({ ok: false, kind: 'serialization', error: serErr }));
        }
        // 循环引用等 stringify 失败 → serialization（区别于检测代码自身抛错 → runtime）
        let json;
        try {
          // 值序列化同入净化窗口：官方控制台返回值走 ivm（不经 stringify、
          // 不调玩家 toJSON），净化更贴官方（对抗评审 P2：探针读值不得被污染改写）
          json = safeStringify(v);
        } catch (e) {
          return safeStringify(nullEnv({
            ok: false, kind: 'serialization',
            error: 'Return value is not JSON-serializable: ' + safeErrorText(e, false),
          }));
        }
        // toJSON 返回非字符串（如 undefined）时 stringify 产物不是字符串——失败
        // 发生在返回值序列化阶段，按契约归 serialization（不得漏到 JSON.parse 再
        // 被"not valid JSON"误报 runtime）
        if (typeof json !== 'string') {
          return safeStringify(nullEnv({
            ok: false, kind: 'serialization',
            error: 'Return value is not JSON-serializable: toJSON produced ' + typeof json,
          }));
        }
        // 值规范化往返的 parse 走预置捕获（覆写全局 parse 可伪造 {ok:true,value}
        // 的 value——runExpr 是验收探针读值通道，自审 P2）
        return safeStringify(nullEnv({ ok: true, value: parse(json) }));
      } catch (e) {
        // 字面量终极兜底（对抗评审 P1）：sanitizedStringify 自身失败时不得二次抛
        return '{"ok":false,"kind":"runtime","error":"runExpr 出口序列化失败"}';
      }
    },
    /** tick 末：Memory 写回 RawMemory + 本 tick intentsList 一次性带出（官方
     *  run 闭包尾 outMessage 同构，runtime.js:271-296——intents 与 memory 同批
     *  传出）。返回 JSON 字符串（宿主 exec_json 约定）：
     *  {ok:true, intentsList, intentsCpu} | {ok:false, error, intentsList, intentsCpu}。 */
    endTick() {
      // writeBackMemory 整体守卫（对抗评审 P3）：玩家异常对象的 message getter
      // 可在写回 catch 块内再抛——逃逸会被宿主判硬中断并误计费
      let out;
      try {
        out = writeBackMemory();
      } catch (e) {
        out = nullEnv({ ok: false, error: 'Memory 写回阶段异常: ' + safeErrorText(e, false) });
      }
      const payload = intentsPayload();
      if (typeof out === 'object' && out !== null) {
        // error 键统一过闸（B0-3 评审 P1 清偿）：safeErrorText 只覆盖本段拼的
        // 三处，writeBackMemory 内部 catch（game.js 'Memory 不可序列化: '+why）
        // 与未来路径的 error 串在此兜底截断——实测 2MB message 可经 toJSON 抛错
        // 注入（16K 码元上限，防 exec_json 出口跨边界放大）
        if (typeof out.error === 'string') out.error = clipHostError(out.error);
        out.intentsList = payload ? payload.list : null;
        out.intentsCpu = payload ? payload.cpu : 0;
      }
      // 编码守卫（与 writeBackMemory 同构，自审 P2-3）：载荷携带抛错 getter 等
      // 病态形态时 stringify 抛错不得逃逸——逃逸会被宿主判硬中断并误计费。
      // 净化窗口覆盖信封与 intentsList 全图（数组通道同面）
      try {
        return safeStringify(out);
      } catch (e) {
        // 字面量终极兜底（同 runLoop）：ok:false 使宿主按写回失败处置（丢 intents）
        return '{"ok":false,"error":"endTick 序列化失败"}';
      }
    },
    /** 模块表替换：官方 timestamp 变更路径（game.js:507-512）——整体重绑
     *  require + 重建缓存对象（而非逐键 delete：玩家冻结 require.cache 时
     *  delete 会抛 TypeError 打挂 set_modules，重绑新对象天然免疫）。 */
    resetModules() {
      resetForNewModules();
    },
    /** 宿主私有通道：注入 RawMemory 初值（写闭包 rawMemoryData，不经
     *  globalThis.RawMemory——玩家换掉全局绑定后旧路径执行伪造 set，
     *  A5-core 对抗评审 P3b；随 B0 WP4 收口根治）。 */
    setRawMemory(data) {
      setRawMemoryHost(data);
    },
    drainMissing() {
      // 两桶：missing（get-trap 确定缺失）+ hasProbes（has-trap 低置信，可能是否定式探测）
      // 内层桶同样 null 原型——污染可从任意层级的继承 toJSON 触发
      const out = nullEnv({ missing: Object.create(null), hasProbes: Object.create(null) });
      for (const [k, c] of missing) out.missing[k] = c;
      for (const [k, c] of hasProbes) out.hasProbes[k] = c;
      missing.clear();
      hasProbes.clear();
      return safeStringify(out);
    },
    drainStubCalls() {
      const out = nullEnv({});
      for (const [k, c] of stubCalls) out[k] = c;
      stubCalls.clear();
      return safeStringify(out);
    },
  };
  // 宿主通道不上玩家全局（B0 WP4）：__kkp.host 由 Rust 侧捕获句柄，
  // 随后整个 __kkp 连同 host 引用被删除——玩家零宿主控制面

  // ---------------------------------------------------------------- 遮蔽 deno_core 内部面
  // deno_core 的内部函数集合默认对玩家可见——ADR-002 纪律。
  // - Deno.core（约 100 个 op，含可一行 abort 宿主的 op_panic）；
  // - globalThis.__bootstrap：deno_core 0.412 的 bootstrap 命名空间，挂有与
  //   Deno.core 同一个 core 对象及 internals（00_infra.js:549 / 01_core.js:1301），
  //   只删 Deno 会漏掉它。
  // op 引用已捕获进各段闭包，遮蔽不影响宿主通道；遮蔽后的全局面复查
  //（getOwnPropertyNames 白名单比对）见 sandbox 测试——deno_core 升级若新增
  // 残留全局，测试会显形。
  delete globalThis.Deno;
  delete globalThis.__bootstrap;
  // __kkp 的删除不在本段：Rust 侧先从 __kkp.host 捕获私有函数句柄，捕获
  // 完成后执行清理脚本删除 __kkp（先删会把宿主通道一起埋掉）
})();
