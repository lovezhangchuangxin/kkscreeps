// 记录玩家可见的 LINK_CAPACITY 常量：包切换后计划派生常量必须按新
// 计划对已运行玩家生效（z-gate 移除后 700 回落原生 800）。
exports.loop = () => {
  Memory.linkCap = LINK_CAPACITY;
};
