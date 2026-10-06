// 覆盖单目标近战射程：attack.range 1→3。无处理器——只通过冻结规则
// 同时驱动 Rust 结算距离与玩家常量注入（同源生效）。
export default function register(api) {
  api.rules.override('builtin:creep.attack.range', 3);
}
