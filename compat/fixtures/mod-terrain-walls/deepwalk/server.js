// 深行玩法：独立作者以同一公共规则声明墙地形可通行，验证声明面
// 不按包 ID 特判。与 ghost 覆盖同一规则，共装会被 freeze 判覆盖
// 冲突（同规则双包无冗余共装语义）；跨规则组合见 tireless。
export default function register(api) {
  api.rules.override('builtin:terrain.wallPassable', 1);
}
