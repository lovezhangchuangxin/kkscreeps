export default function register(api) {
  const counter = api.objects.define('workload:counter', {
    version: 1, playerClass: 'Counter', visibility: 'room-visible',
    ownerField: 'owner', positionFields: ['x', 'y'], fields: {
      owner: {type: 'string', maxBytes: 128},
      x: {type: 'u32', min: 0, max: 49}, y: {type: 'u32', min: 0, max: 49},
      value: {type: 'i32', default: 0, visibility: 'public'},
    },
  });
  api.intents.define('workload:increment', {
    scope: 'object', target: counter,
    args: {type: 'record', fields: {amount: {type: 'i32', min: 1, max: 1}}},
    process(ctx) { ctx.target.fields.value += ctx.args.amount; },
  });
  api.handlers.wrap('workload:increment.slot', {id: 'count'}, (ctx, next) => {
    next();
    ctx.state = {increments: (ctx.state?.increments || 0) + 1};
  });
}
