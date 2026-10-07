// 意图压制声明包（G07.b）：mark 声明被 bump 压制——同接收者同 tick
// 两条意图齐发时 mark 不执行（引擎按键存在性判定）。两意图都设绝
// 对值，期望不随 tick 累积。
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const counter = api.objects.define('suppress:counter', {
    version: 1,
    playerClass: 'Counter',
    visibility: 'room-visible',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0, visibility: 'public' },
    },
  });
  api.intents.define('suppress:bump', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) { ctx.target.fields.value = ctx.args.amount; },
  });
  api.intents.define('suppress:mark', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { mark: { type: 'i32', min: 0, max: 9 } } },
    suppressedBy: ['suppress:bump'],
    process(ctx) { ctx.target.fields.value = ctx.args.mark; },
  });
}
