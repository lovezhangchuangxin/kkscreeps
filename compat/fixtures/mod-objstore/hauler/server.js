// 独立库存对象包：objstore-hauler:crate 声明 300 总容量（创建即携带
// store/storeCapacity 信封，库存只能经资源动作变动）；objstore-hauler:marker
// 不声明容量，作为「无信封不可转移」对照。对象 tick 经 ctx.target.fields
// 读同一 payload（含库存信封），把执行期读值记入 namespace state。
export default function register(api) {
  api.objects.define('objstore-hauler:crate', {
    version: 1,
    playerClass: 'HaulerCrate',
    positionFields: ['x', 'y'],
    capacity: 300,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      ctx.state = { energy: ctx.target.fields.store.energy };
    },
  });
  api.objects.define('objstore-hauler:marker', {
    version: 1,
    playerClass: 'HaulerMarker',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
  });
}
