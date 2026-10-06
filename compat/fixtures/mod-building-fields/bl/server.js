// G08.a 建筑字段与生命周期：bl:gauge 声明 heat/cycles 字段（默认 7/0）、
// 周期生产（每 tick 产 5 能量）与 tick 槽。先产后 tick：tick 读到的
// store.energy 是本轮生产后的候选值；自定义键整写、原生键只读。
export default function register(api) {
  api.buildings.define('bl:gauge', {
    playerClass: 'Gauge',
    cost: { energy: 100 },
    hitsMax: 500,
    pathing: 'obstacle',
    capacity: 200,
    production: { interval: 1, inputs: {}, outputs: { energy: 5 } },
    fields: {
      heat: { type: 'u32', min: 0, max: 1000, default: 7 },
      cycles: { type: 'u32', min: 0, max: 100000, default: 0 },
    },
    tick(ctx) {
      ctx.target.fields['bl:heat'] = (ctx.target.fields['bl:heat'] || 0) + 1;
      ctx.target.fields['bl:cycles'] = (ctx.target.fields['bl:cycles'] || 0) + 1;
    },
  });
}
