import { createApi } from '@kkscreeps/mod';

export default function register(api) {
  const widget = api.objects.define('demo-base:widget', {
    version: 2,
    playerClass: 'Widget',
    visibility: 'room-visible',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      charge: { type: 'i32', default: 0, visibility: 'owner' },
    },
  });
  api.intents.define('demo-base:recharge', {
    scope: 'object',
    target: widget,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 100 } } },
    process(ctx) {
      const fields = ctx.target.fields;
      fields.charge = (fields.charge || 0) + ctx.args.amount;
    },
  });
  api.handlers.after('demo-base:recharge.slot', { id: 'recharge-audit' }, function audit() {});
  api.rules.define('demo-base:recharge.ceiling', {
    type: 'i32',
    min: 1,
    max: 10000,
    default: 1000,
    consumers: ['demo-base:handler.recharge-audit'],
  });
}
