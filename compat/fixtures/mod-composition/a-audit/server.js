// 处理器符号依赖允许指向稍后注册的包；不依赖目录或字典序碰巧正确。
export default function register(api) {
  for (const kind of ['before', 'wrap', 'after']) {
    api.handlers[kind](
      'base:counter.tick',
      { id: kind, dependsOn: ['z-gate:handler.' + kind] },
      (ctx, next) => {
        if (kind === 'before') ctx.target.fields.trace += 'A';
        if (kind === 'wrap') {
          ctx.target.fields.trace += '[';
          next();
          ctx.target.fields.trace += ']';
        }
        if (kind === 'after') ctx.target.fields.trace += 'a';
      },
    );
  }
}
