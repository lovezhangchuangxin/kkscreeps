// 幽灵地形玩法：声明墙地形可通行。无处理器——冻结规则同时驱动
// 仲裁静态障碍的墙项（引擎 Plane）与玩家面原生寻路地形（打包时清
// 墙位，wall|swamp 保留沼泽语义）；物理地形查询不随本声明改变。
export default function register(api) {
  api.rules.override('builtin:terrain.wallPassable', 1);
}
