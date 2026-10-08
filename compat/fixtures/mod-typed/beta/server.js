// 类型生成夹具（beta）：归属者集合可见对象（owner-collection 须声明
// 归属字段）与可空参数意图（args nullable → 声明为可选参数）；与
// alpha 互不依赖，选择门互不残留。
export default function register(api) {
  const relay = api.objects.define('beta:relay', {
    version: 1,
    playerClass: 'TypedRelay',
    visibility: 'owner-collection',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128 },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      charge: { type: 'u64', max: 1_000_000, default: 0 },
      mode: { type: 'enum', values: ['listen', 'send'], default: 'listen' },
      history: { type: 'array', element: { type: 'i32' }, maxLen: 100 },
      labels: { type: 'map', value: { type: 'string', maxBytes: 64, nullable: true }, maxLen: 16 },
      audit: { type: 'string', maxBytes: 512, visibility: 'server-only' },
    },
  });
  api.intents.define('beta:ping', {
    scope: 'object',
    target: relay,
    args: {
      type: 'record',
      fields: { code: { type: 'enum', values: ['echo', 'sync'] } },
      nullable: true,
    },
    process(ctx) {
      ctx.target.fields.history = [
        ...(ctx.target.fields.history ?? []),
        ctx.args?.code === 'sync' ? 1 : 0,
      ];
    },
  });
  api.resources.define('beta:spark');
}
