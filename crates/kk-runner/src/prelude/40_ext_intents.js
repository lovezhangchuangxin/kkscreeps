// 扩展意图的玩家面原语：把 Mod 声明动作记入 intentsList 的对象容器
// （list[targetId][intentKey] = args）。本段仅在宿主声明分片含 Mod 意图时
// 装配（SandboxConfig.extension_intents）——vanilla 分片的全局面保持不变。
// 此处只做记录与计费：键是否在冻结目录、参数 schema、接收者与所有权
// 全部由宿主结算侧独立校验，伪造目标或参数最多得到一次不执行。Mod 的
// 玩家 SDK 在此之上安装类方法（emit 的公开形态随玩家模块装载器提供）。
// 运行时点在 __kkp 删除之前，gw 经闭包捕获后不再依赖全局容器。
;(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  Object.defineProperty(globalThis, '__kkEmitExtensionIntent', {
    configurable: false,
    writable: false,
    enumerable: false,
    value: function __kkEmitExtensionIntent(intentKey, targetId, args) {
      if (typeof intentKey !== 'string' || typeof targetId !== 'string') return false;
      // undefined 归一为 null：undefined 无法经出站 stringify 存活（会静默
      // 丢意图），null 是 nullable 参数 schema 的合法形态。
      gw.intents.setExt(targetId, intentKey, args === undefined ? null : args);
      return true;
    },
  });
})();
