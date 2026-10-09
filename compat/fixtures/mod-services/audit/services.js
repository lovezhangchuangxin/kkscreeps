export default function register(api) {
  let calls = 0;
  api.services.define('audit:summary', {
    async start(ctx) {
      return { sample: await ctx.probe('audit:sample'), tick: ctx.view.time };
    },
    async request(ctx, args) {
      calls++;
      return {
        calls,
        tick: ctx.view.time,
        rooms: Object.keys(ctx.view.rooms),
        label: args.label,
        sample: await ctx.probe('audit:sample'),
      };
    },
    stop(ctx) {
      return { calls, tick: ctx.view.time };
    },
  });
}
