// 计划级声明只改变通行，物理对象、放置检查与生物占格保持。
export default function register(api) {
  api.rules.override('builtin:movement.obstaclesPassable', 1);
}
