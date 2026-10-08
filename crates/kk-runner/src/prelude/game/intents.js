/* 玩家指令清单与显示侧虚拟计费；宿主仍按最终清单重新计费。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const __Object = Object;
  // 每 tick 重建闭包内的指令清单：对象动作按 id/动作名覆盖，重复设置不重复计费；
  // room/global 动作按数组元素计费。say/pull 不收费，remove 统一减 0.2。
  // JS cpu 供 getUsed 显示，Rust 在 end_tick 按最终保留清单重新计算费用。
  // 来源：上游 driver/lib/runtime/runtime.js 的 intents 与 make.js 的 intentsCount。
  const FREE_INTENT_NAMES = __Object.create(null);
  FREE_INTENT_NAMES.say = true;
  FREE_INTENT_NAMES.pull = true;
  function resetIntents() {
    const list = __Object.create(null);
    let cpu = 0;
    gw.intents = {
      list,
      set(id, name, data) {
        list[id] = list[id] || __Object.create(null);
        if (!FREE_INTENT_NAMES[name] && !list[id][name]) cpu += 0.2;
        // egress data 剥原型：官方出口是 ExternalCopy 结构拷贝不经 stringify
        //（runtime.js:296），我方出口是 stringify——继承的 toJSON 会在序列化时
        // 改写载荷，剥原型是等价结构位（官方对注入侧同样剥顶层，:56）。
        // 数组元素经出口净化窗口兜底（sanitizedStringify）
        __Object.setPrototypeOf(data, null);
        list[id][name] = data;
      },
      // 扩展意图专用：载荷原样保留（null/原始值都合法——参数 schema 允许
      // nullable），不做原型剥离。继承的 toJSON 可改写出站载荷——与完整
      // 性无关（args 本就由玩家全权控制），结构校验在宿主结算侧按冻结
      // 计划独立进行。计费按键存在性判定（首次发射计 0.2），与宿主侧
      // "每键一次"同轨，不因载荷为 null/0/false 等假值重复计费。
      setExt(id, name, data) {
        list[id] = list[id] || __Object.create(null);
        if (list[id][name] === undefined) cpu += 0.2;
        list[id][name] = data;
      },
      // 全局 intent 通道（官方 runtime.js:78-86 逐字）：list 顶层按动作名
      // 键的数组（非对象 id 路由）；maxLen 上限返 false（Game.notify 20——
      // 每 tick 上限，beginTick 重建 list 即重置）
      push(name, data, maxLen) {
        list[name] = list[name] || [];
        if (maxLen && list[name].length >= maxLen) {
          return false;
        }
        list[name].push(data);
        cpu += 0.2;
        return true;
      },
      pushByName(id, name, data, maxLen) {
        list[id] = list[id] || __Object.create(null);
        __Object.setPrototypeOf(data, null);
        list[id][name] = list[id][name] || [];
        // 官方 runtime.js:86-88 maxLen 门：满返 false（market deal 10 /
        // cancelOrder·changeOrderPrice·extendOrder 50——2B 批 P2 消费）
        if (maxLen && list[id][name].length >= maxLen) {
          return false;
        }
        list[id][name].push(data);
        // 官方 room/global 级按元素数计费（make.js:100-106；免费表 say/pull
        // 是对象级语义）——Rust 侧 take_endtick_intents 同口径兜底，此处计入
        // 玩家可见的 getUsed（intents.cpu 显示面）
        cpu += 0.2;
        return true;
      },
      remove(id, name) {
        if (list[id] && list[id][name] !== undefined) {
          delete list[id][name];
          cpu -= 0.2;
          return true;
        }
        return false;
      },
      get cpu() {
        return cpu;
      },
    };
  }
  /** 官方 run 闭包尾等价物：intentsList 原样带出（runtime.js:274-296）。 */
  function intentsPayload() {
    return gw.intents && { list: gw.intents.list, cpu: gw.intents.cpu };
  }

  globalThis.__kkp.intentCollector = { resetIntents, intentsPayload };
})();
