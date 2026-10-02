/* 注册 Rust 使用的生命周期函数：注入世界、执行玩家代码、写回与导出诊断。
 * 函数先保存到 __kkp.host，由 Rust 捕获 V8 句柄后删除临时全局 __kkp。
 * 本段删除 Deno 与 __bootstrap；各段此前捕获的 op 引用仍可在闭包中使用。
 * 宿主返回值使用捕获的 JSON 函数编码，普通玩家错误由结果字段报告。
 */
(() => {
  'use strict';

  // 捕获 JSON.parse 和 eval；消息序列化委托给 gw.sanitizedStringify。
  // 它只临时移除 Object/Array 根原型上的 toJSON，不能阻止所有玩家回调；
  // Memory 写回仍按原生 JSON.stringify 语义调用玩家自己的 toJSON。
  const stringify = JSON.stringify;
  const parse = JSON.parse;
  const __eval = eval;
  const gw = globalThis.__kkp.gw;
  const nullEnv = gw.nullEnv;
  const safeStringify = gw.sanitizedStringify;
  const hostObject = gw.__Object;
  // 错误文本最多保留 16K UTF-16 码元，再附截断提示；此限制不是 UTF-8 字节数。
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
  const { beginTick, beginTickKkpv, writeBackMemory, intentsPayload, setRawMemoryHost, rawMemoryDataHost,
    setSegmentsWindowHost, setForeignSegmentHost, segmentsOutHost } = globalThis.__kkp.memoryWorld;

  globalThis.__kkp.host = {
    /** 每 tick 注入世界快照并重建 Game。cpuMeta：宿主预算真值（Rust 侧
     * begin_tick 第二参数——避免整世界深拷贝覆写三键，JS 侧合并覆盖占位基准）。 */
    beginTick(worldJson, cpuMeta) {
      beginTick(worldJson, cpuMeta);
    },
    /** KKPV 通道注入：世界信封 JSON 串 + 私有对象束 ArrayBuffer + 公开
     * 地形束 ArrayBuffer（共享内存零拷贝挂载）+ 预算真值；对象记录与
     * 地形视图在沙箱内从两束解码。 */
    beginTickKkpv(worldJson, bundle, publicBundle, cpuMeta) {
      beginTickKkpv(worldJson, bundle, publicBundle, cpuMeta);
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
        // 结果编码失败时返回固定 JSON，避免再次调用可能失败的序列化函数。
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
        // 按异常对象分类：SyntaxError 记为 syntax，其他异常记为 runtime。
        // 这里也会将执行过程中显式抛出的 SyntaxError 归为 syntax，未单独解析代码。
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
            // 构造器名称可能来自玩家 getter，错误文本也需限制长度。
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
          // 返回值序列化暂时忽略 Object/Array 根原型的 toJSON；对象自己的 toJSON 仍可能执行。
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
        // 使用捕获的 parse 还原 JSON 值，避免玩家替换全局 parse 后改写返回值。
        return safeStringify(nullEnv({ ok: true, value: parse(json) }));
      } catch (e) {
        // 编码失败时返回固定错误 JSON，避免异常逃出后被 Rust 归为硬中断。
        return '{"ok":false,"kind":"runtime","error":"runExpr 出口序列化失败"}';
      }
    },
    /** tick 末：Memory 写回 RawMemory + 本 tick intentsList 一次性带出（官方
     *  run 闭包尾 outMessage 同构，runtime.js:271-296——intents 与 memory 同批
     *  传出）。返回 JSON 字符串（宿主 exec_json 约定）：
     *  {ok:true, intentsList, intentsCpu} | {ok:false, error, intentsList, intentsCpu}。 */
    endTick() {
      // 写回错误处理也可能读取玩家异常对象的 getter；捕获再次抛出的错误，返回普通写回失败。
      let out;
      try {
        out = writeBackMemory();
      } catch (e) {
        out = nullEnv({ ok: false, error: 'Memory 写回阶段异常: ' + safeErrorText(e, false) });
      }
      const payload = intentsPayload();
      if (typeof out === 'object' && out !== null) {
        // 统一限制写回结果的 error 文本，覆盖 game.js 内部生成的错误；上限为 UTF-16 码元数。
        if (typeof out.error === 'string') out.error = clipHostError(out.error);
        out.intentsList = payload ? payload.list : null;
        out.intentsCpu = payload ? payload.cpu : 0;
        // 仅在写回成功且当前 RawMemory 是原始 string 时携带候选，供世界提交后保存。
        // String 包装对象可能带自定义 toJSON，因此不通过这个导出检查。
        if (out.ok === true) {
          const rm = rawMemoryDataHost();
          if (typeof rm === 'string') out.rawMemory = rm;
          // 同时带出活动 segment 窗口写入，以及 active/public/defaultPublic/foreign 请求。
          const so = segmentsOutHost();
          if (so !== null && so !== undefined) out.segmentsOut = so;
        }
      }
      // 载荷中的 getter 或 toJSON 可能抛错；转换成普通写回失败，避免被宿主归为硬中断。
      try {
        return safeStringify(out);
      } catch (e) {
        // 字面量终极兜底（同 runLoop）：ok:false 使宿主按写回失败处置（丢 intents）
        return '{"ok":false,"error":"endTick 序列化失败"}';
      }
    },
    /** 模块表替换后整体重建 require 与缓存。
     * 不能逐键删除旧缓存，因为玩家可以冻结旧缓存对象。 */
    resetModules() {
      resetForNewModules();
    },
    /** 通过闭包设置 RawMemory 初值，不调用玩家可替换的全局 RawMemory.set。 */
    setRawMemory(data) {
      setRawMemoryHost(data);
    },
    /** 在 beginTick 前注入 RawMemory、活动 segment 窗口和 foreignSegment。 */
    setMemoryState(data, segmentsJson, foreignJson) {
      setRawMemoryHost(data);
      setSegmentsWindowHost(segmentsJson);
      setForeignSegmentHost(foreignJson);
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
  // Rust 捕获 host 函数句柄后删除 __kkp，后续通过句柄调用生命周期函数。

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
