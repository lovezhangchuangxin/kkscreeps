// 类型生成夹具（alpha）：覆盖全部 ValueKind 的自有对象、对象域意图、
// 命名空间资源与建筑声明；与 beta 互不依赖，供 --select 选择门验收。
// 字段可见性三态齐全（public/owner/server-only），归属者字段经 owner
// 字段判定；信封同名位置字段 x/y 由投影最后写入等值覆写。
export default function register(api) {
  const beacon = api.objects.define('alpha:beacon', {
    version: 1,
    playerClass: 'TypedBeacon',
    visibility: 'room-visible',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128 },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      level: { type: 'i32', min: -5, max: 10, default: 0 },
      state: { type: 'enum', values: ['on', 'off'], default: 'off' },
      armed: { type: 'bool', default: false },
      visitors: {
        type: 'array',
        element: { type: 'string', maxBytes: 32, nullable: true },
        maxLen: 20,
      },
      tally: { type: 'map', value: { type: 'i64' }, maxLen: 64 },
      mentor: { type: 'ref', target: 'alpha:beacon', version: 1, nullable: true },
      note: { type: 'string', maxBytes: 256, visibility: 'owner' },
      secret: { type: 'string', maxBytes: 256, visibility: 'server-only' },
    },
    tick(ctx) {
      ctx.target.fields.state = ctx.target.fields.state === 'on' ? 'off' : 'on';
    },
  });
  api.intents.define('alpha:salute', {
    scope: 'object',
    target: beacon,
    args: {
      type: 'record',
      fields: {
        amount: { type: 'i32', min: 1, max: 50 },
        tag: { type: 'string', maxBytes: 32, nullable: true },
      },
      optional: ['tag'],
    },
    process(ctx) {
      ctx.target.fields.level = Math.min(10, (ctx.target.fields.level ?? 0) + ctx.args.amount);
    },
  });
  api.resources.define('alpha:fuel');
  api.buildings.define('alpha:gate', {
    playerClass: 'TypedGate',
    cost: { energy: 500, 'alpha:fuel': 50 },
    hitsMax: 1000,
    pathing: 'obstacle',
  });
}
