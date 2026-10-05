// 覆盖 base 的节奏/增益规则与内建 link 容量；冻结后 base 的执行期读取
// 必须看到这些覆盖值，即使 base 声明的默认值不同。
export default function register(api) {
  api.rules.override('base:period', 2);
  api.rules.override('base:rate', 0.25);
  api.rules.override('builtin:link.capacity', 700);
}
