import { createApi } from '@kkscreeps/mod';
import { label } from './util/label.js';

export default function register(api) {
  const start = (api.config && api.config.start) || 0;
  if (label('demo-counter') !== 'demo-counter') {
    throw new Error('util 相对导入失效');
  }
  const counter = api.objects.define('demo-counter:counter', {
    version: 1,
    playerClass: 'Counter',
    visibility: 'room-visible',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: start, visibility: 'public' },
    },
  });
  api.intents.define('demo-counter:increment', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) {
      const fields = ctx.target.fields;
      fields.value = (fields.value || 0) + ctx.args.amount;
    },
  });
  api.mounts.define({ key: 'demo-base:widget', version: 2 }, 'counterTag', {
    type: 'string',
    maxBytes: 32,
    nullable: true,
  });
  api.handlers.wrap('builtin:link.tick', { id: 'source-audit' }, function audit() {});
  api.rules.override('builtin:link.capacity', 700);
}
