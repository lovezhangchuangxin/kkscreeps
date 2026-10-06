// G08.a 独立第二包：bk:beacon 无生产、无仓库，只有字段与 tick——证明
// 字段域不绑定单一命名空间，也不依赖生产面。signal 每轮翻转。
export default function register(api) {
  api.buildings.define('bk:beacon', {
    playerClass: 'Beacon',
    cost: { energy: 50 },
    hitsMax: 100,
    pathing: 'plain',
    fields: {
      signal: { type: 'u32', min: 0, max: 1, default: 0 },
      flashes: { type: 'u32', min: 0, max: 100000, default: 0 },
    },
    tick(ctx) {
      ctx.target.fields['bk:signal'] = ctx.target.fields['bk:signal'] ? 0 : 1;
      ctx.target.fields['bk:flashes'] = (ctx.target.fields['bk:flashes'] || 0) + 1;
    },
  });
}
