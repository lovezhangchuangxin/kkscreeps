// gamma 与 alpha/beta 无 manifest 依赖：用于验证与另一个包组合安装。
// 只在 link tick 上写自有 state，不提供玩家面。
export default function register(api) {
  api.handlers.before('builtin:link.tick', { id: 'mark' }, ctx => {
    ctx.state = { marked: true };
  });
}
