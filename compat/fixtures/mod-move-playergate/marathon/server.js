// 覆盖移动玩家门的疲劳上界：fatigueLimit 0→u32::MAX。无处理器——只
// 通过冻结规则同时驱动玩家面 move()/moveTo() 预检常量与仲裁原生准入
// 预填（同源生效），疲劳移动者不再需要停歇。
export default function register(api) {
  api.rules.override('builtin:creep.move.fatigueLimit', 4294967295);
}
