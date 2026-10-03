// 每个链位累计一次；native 中心只减冷却，四个计数用于识别空转或重复续行。
export default function register(api) {
  const count = (ctx, key) => {
    ctx.state ??= {};
    ctx.state[key] = (ctx.state[key] || 0) + 1;
  };
  api.handlers.before('builtin:link.tick', { id: 'before' }, ctx => count(ctx, 'before'));
  api.handlers.wrap('builtin:link.tick', { id: 'a_outer' }, (ctx, next) => {
    count(ctx, 'outer');
    next();
  });
  api.handlers.wrap('builtin:link.tick', { id: 'b_inner' }, (ctx, next) => {
    count(ctx, 'inner');
    next();
  });
  api.handlers.after('builtin:link.tick', { id: 'after' }, ctx => count(ctx, 'after'));
}
