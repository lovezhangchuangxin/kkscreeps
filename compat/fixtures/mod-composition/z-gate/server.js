export default function register(api) {
  api.rules.override('builtin:link.capacity', 700);
  api.handlers.before('base:counter.tick', { id: 'before' }, (ctx) => {
    ctx.target.fields.trace = 'Z';
  });
  api.handlers.wrap('base:counter.tick', { id: 'wrap' }, (ctx, next) => {
    ctx.target.fields.trace += '(';
    next();
    ctx.target.fields.trace += ')';
  });
  api.handlers.after('base:counter.tick', { id: 'after' }, (ctx) => {
    ctx.target.fields.trace += 'z';
  });
}
