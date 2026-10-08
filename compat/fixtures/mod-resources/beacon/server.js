// 组合夹具：manifest 依赖 ore（选择 beacon 会自动加入 ore），
// 自身声明另一个命名空间资源，并在 link tick 写自有 state，
// 验证资源目录与已有处理链/规则在同一计划内共存。
export default function register(api) {
  api.resources.define('beacon:spark');
  api.handlers.before('builtin:link.tick', { id: 'mark' }, (ctx) => {
    ctx.state = { marked: (ctx.state?.marked ?? 0) + 1 };
  });
}
