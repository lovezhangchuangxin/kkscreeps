// 对照包：与 suppressed 同形但不声明压制——两条意图都按键序执行
//（bump 先置 3、mark 后置 7），证明场景差异来自压制声明本身。
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const counter = api.objects.define('free:counter', {
    version: 1,
    // 玩家类名与 suppress 包区分：组合选择下玩家全局绑定须唯一。
    playerClass: 'FreeCounter',
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
  api.intents.define('free:bump', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) { ctx.target.fields.value = ctx.args.amount; },
  });
  api.intents.define('free:mark', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { mark: { type: 'i32', min: 0, max: 9 } } },
    process(ctx) { ctx.target.fields.value = ctx.args.mark; },
  });
}
