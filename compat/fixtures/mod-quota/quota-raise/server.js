// 独立配额抬高包：tower 的 RCL 8 配额 6 → 20。只声明规则覆盖，
// 不携带对象或处理器；玩家预检（CONTROLLER_STRUCTURES 注入）与
// 结算配额快照读同一冻结值。
export default function register(api) {
  api.rules.override('builtin:structure.quota.tower.rcl8', 20);
}
