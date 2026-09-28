/* kk-runner 桩环境 prelude · 段 4（末段）：宿主通道 + 遮蔽。
 *
 * __kk 是宿主驱动的 tick 通道（beginTick/runLoop/runExpr/endTick/仪表排干），
 * 冻结后不可枚举不可改写。已知残余：冻结防不住玩家直接调用 __kk.drainMissing()
 * 抢先清空仪表，根治需宿主侧闭包/独立 op——M2 玩家 API 层重写，已登记
 * compatibility.md 灰区表。
 * 本段末尾遮蔽 deno_core 内部面并删除 __kkp（段间私有上下文）——此后玩家代码
 * 才开始执行。
 */
(() => {
  'use strict';

  // 预置 JSON 函数（官方 systemFunctions.JSON 同款语义）：宿主消息编码不得
  // 经玩家可覆写的全局——覆写全局 stringify 曾可伪造 intentsList（外部复审
  // P1 实证：无 API 调用造出工地）。玩家自己的代码仍用全局 JSON，不受影响。
  const stringify = JSON.stringify;

  const { missing, hasProbes, stubCalls } = globalThis.__kkp;
  const { require: requireFn, moduleCache, lodash: _ } = globalThis.__kkp.cjs;
  const { beginTick, writeBackMemory, intentsPayload } = globalThis.__kkp.memoryWorld;

  globalThis.__kk = {
    /** 每 tick 注入世界快照并重建 Game。 */
    beginTick(worldJson) {
      beginTick(worldJson);
    },
    /** 执行玩家入口：require('main').loop()。返回结果 JSON 字符串（宿主 exec_json 约定）。 */
    runLoop() {
      let out;
      try {
        const main = requireFn('main');
        if (main && typeof main.loop === 'function') {
          main.loop();
          out = { ok: true };
        } else {
          // 官方 game.js:555-563 run()：`if(isObject && isFunction(loop)) {...}` 无
          // else——未导出 loop() 的 main 静默跳过，不是玩家错误（不打 console、
          // 不计入错误统计；bot-check 的 errored_ticks 口径据此）
          out = { ok: true };
        }
      } catch (e) {
        // 官方行为：未捕获的玩家错误打进 console（bot-check 据此采集每 tick 错误流）
        console.error(String((e && e.stack) || e));
        out = { ok: false, kind: 'runtime', error: String((e && e.stack) || e) };
      }
      try {
        return stringify(out);
      } catch (e) {
        return stringify({ ok: true, note: 'loop 完成（结果不可序列化）' });
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
        v = (0, eval)(code);
      } catch (e) {
        // eval 在全局作用域抛出的 SyntaxError 即解析期错误（官方控制台同口径）；
        // 运行期错误（含运行期动态构造的语法异常）归 runtime
        const kind = e instanceof SyntaxError ? 'syntax' : 'runtime';
        out = { ok: false, kind, error: String((e && e.message) || e) };
        return stringify(out);
      }
      if (v === undefined) {
        return stringify({ ok: true, value: null });
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
            const ctorName = (ctor && typeof ctor === 'object' && ctor.name) || 'non-plain';
            serErr = 'Return value is a ' + ctorName + ' object, not a plain JSON value';
          }
        }
        if (serErr !== null) {
          return stringify({ ok: false, kind: 'serialization', error: serErr });
        }
        // 循环引用等 stringify 失败 → serialization（区别于检测代码自身抛错 → runtime）
        let json;
        try {
          json = stringify(v);
        } catch (e) {
          return stringify({
            ok: false, kind: 'serialization',
            error: 'Return value is not JSON-serializable: ' + (e && e.message),
          });
        }
        // toJSON 返回非字符串（如 undefined）时 stringify 产物不是字符串——失败
        // 发生在返回值序列化阶段，按契约归 serialization（不得漏到 JSON.parse 再
        // 被"not valid JSON"误报 runtime）
        if (typeof json !== 'string') {
          return stringify({
            ok: false, kind: 'serialization',
            error: 'Return value is not JSON-serializable: toJSON produced ' + typeof json,
          });
        }
        return stringify({ ok: true, value: JSON.parse(json) });
      } catch (e) {
        return stringify({ ok: false, kind: 'runtime', error: String((e && e.message) || e) });
      }
    },
    /** tick 末：Memory 写回 RawMemory + 本 tick intentsList 一次性带出（官方
     *  run 闭包尾 outMessage 同构，runtime.js:271-296——intents 与 memory 同批
     *  传出）。返回 JSON 字符串（宿主 exec_json 约定）：
     *  {ok:true, intentsList, intentsCpu} | {ok:false, error, intentsList, intentsCpu}。 */
    endTick() {
      const out = writeBackMemory();
      const payload = intentsPayload();
      if (typeof out === 'object' && out !== null) {
        out.intentsList = payload ? payload.list : null;
        out.intentsCpu = payload ? payload.cpu : 0;
      }
      return stringify(out);
    },
    /** 清空 require 缓存（玩家代码更新/换模块表时）。lodash 预置保留。 */
    resetModules() {
      for (const k of Object.keys(moduleCache)) delete moduleCache[k];
      moduleCache.lodash = { exports: _ };
    },
    drainMissing() {
      // 两桶：missing（get-trap 确定缺失）+ hasProbes（has-trap 低置信，可能是否定式探测）
      const out = { missing: {}, hasProbes: {} };
      for (const [k, c] of missing) out.missing[k] = c;
      for (const [k, c] of hasProbes) out.hasProbes[k] = c;
      missing.clear();
      hasProbes.clear();
      return stringify(out);
    },
    drainStubCalls() {
      const out = {};
      for (const [k, c] of stubCalls) out[k] = c;
      stubCalls.clear();
      return stringify(out);
    },
  };
  // __kk 不可枚举（玩家代码不应该把它当 API 面）且整体冻结——防止玩家覆盖
  // drainMissing 造假验收仪表、把 beginTick/runLoop 换成抛错函数劫持宿主通道。
  Object.freeze(globalThis.__kk);
  Object.defineProperty(globalThis, '__kk', { enumerable: false, writable: false, configurable: false });

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
  delete globalThis.__kkp;
})();
