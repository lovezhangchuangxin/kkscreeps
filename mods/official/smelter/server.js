// 官方生产建筑内容包（公共 SDK，无特权）：声明 smelter:ore / smelter:ingot
// 资源与 smelter:foundry 熔炉——多资源建造成本、周期生产配方全部经
// api.buildings 声明，由宿主通用执行器结算（工地、建造、耐久、库存、
// 生产与寻路），本包不提供任何处理器。
export default function register(api) {
  const config = api.config ?? {};
  if (typeof config !== 'object' || Array.isArray(config)
    || Object.keys(config).some(key => !['interval', 'yield'].includes(key))) {
    throw new Error('smelter config accepts only interval and yield');
  }
  const interval = config.interval ?? 12;
  const yieldPerCycle = config.yield ?? 10;
  if (!Number.isInteger(interval) || interval < 2 || interval > 1000
    || !Number.isInteger(yieldPerCycle) || yieldPerCycle < 1 || yieldPerCycle > 1000) {
    throw new Error('smelter interval must be 2..1000 and yield must be 1..1000');
  }
  api.resources.define('smelter:ore');
  api.resources.define('smelter:ingot');
  api.buildings.define('smelter:foundry', {
    playerClass: 'SmelterFoundry',
    cost: { energy: 5000, 'smelter:ore': 200 },
    hitsMax: 5000,
    pathing: 'obstacle',
    capacity: 1000,
    production: {
      interval,
      inputs: { energy: 60, 'smelter:ore': 30 },
      outputs: { 'smelter:ingot': yieldPerCycle },
    },
  });
}
