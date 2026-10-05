// N2a 资源目录夹具：声明两个命名空间资源，并覆盖 Generic 容器容量，
// 验证新资源的库存表示、容量与搬运掉落都沿既有规则面（无专用 Rust 分支）。
export default function register(api) {
  api.resources.define('ore:crystal');
  api.resources.define('ore:dust');
  // 容器容量沿仓库规则面：JS 覆盖为 50（Signed 域，无 u32 域约束问题）。
  api.rules.override('builtin:store.container.capacity', 50);
}
