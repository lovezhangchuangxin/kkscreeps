// 第二个独立库存对象包（与 hauler 不同类型键与容量，可同计划共存）：
// objstore-vault:cache 声明 100 总容量。与 hauler 包零共享，验证通用能力复用。
export default function register(api) {
  api.objects.define('objstore-vault:cache', {
    version: 1,
    playerClass: 'VaultCache',
    positionFields: ['x', 'y'],
    capacity: 100,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      ctx.state = { energy: ctx.target.fields.store.energy };
    },
  });
}
