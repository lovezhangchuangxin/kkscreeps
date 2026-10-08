// 规则变体夹具包：与 smelter 同一通用接口、不同规则——单资源成本、
// 道路寻路语义（可通行且按道路成本）、免费生产（无输入、产能量）、
// 更低耐久。用于证明建筑面没有针对首个样例硬编码。
export default function register(api) {
  const config = api.config ?? {};
  if (
    typeof config !== 'object' ||
    Array.isArray(config) ||
    Object.keys(config).some((key) => !['interval', 'yield'].includes(key))
  ) {
    throw new Error('panel config accepts only interval and yield');
  }
  const interval = config.interval ?? 20;
  const yieldPerCycle = config.yield ?? 40;
  if (
    !Number.isInteger(interval) ||
    interval < 2 ||
    interval > 1000 ||
    !Number.isInteger(yieldPerCycle) ||
    yieldPerCycle < 1 ||
    yieldPerCycle > 1000
  ) {
    throw new Error('panel interval must be 2..1000 and yield must be 1..1000');
  }
  api.buildings.define('panel:collector', {
    playerClass: 'PanelCollector',
    cost: { energy: 800 },
    hitsMax: 300,
    pathing: 'road',
    capacity: 200,
    production: {
      interval,
      inputs: {},
      outputs: { energy: yieldPerCycle },
    },
  });
}
