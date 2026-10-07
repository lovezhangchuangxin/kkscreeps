// 组合配套包：覆盖不同轴的移动玩家门规则，验证地形通行声明与
// 其他规则覆盖可同计划共存（同规则双包被 freeze 有意互斥，即使值
// 相同——单轴规则的冗余声明不能共装，见 README）。
export default function register(api) {
  api.rules.override('builtin:creep.move.fatigueLimit', 4294967295);
}
