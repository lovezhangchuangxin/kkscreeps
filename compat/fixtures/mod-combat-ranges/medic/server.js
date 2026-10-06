// 覆盖治疗族射程：heal.range 1→3、rangedHeal.range 3→5。与 reach 包
// 独立；两族规则互不牵连（近战射程不受本包影响）。
export default function register(api) {
  api.rules.override('builtin:creep.heal.range', 3);
  api.rules.override('builtin:creep.rangedHeal.range', 5);
}
