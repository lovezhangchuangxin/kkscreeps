// 独立配额压低包：constructedWall 的 RCL 8 配额 2500 → 3。与抬高包
// 覆盖不同规则键，可同计划共存；同键双包覆盖才是冲突。
export default function register(api) {
  api.rules.override('builtin:structure.quota.constructedWall.rcl8', 3);
}
