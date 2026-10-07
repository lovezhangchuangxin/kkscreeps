// 覆盖移动玩家门的 MOVE 部件门槛：minActiveMoves 1→0。无处理器——
// 无 MOVE 部件的 creep 可经方向形态自发移动（疲劳照常累积且永不
// 恢复，代价由玩法自担）。
export default function register(api) {
  api.rules.override('builtin:creep.move.minActiveMoves', 0);
}
