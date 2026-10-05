// 熔炉的玩家面：绑定 Structure 子类并提供类型常量。完工结构以
// type=structure 记录进 FIND_STRUCTURES；本类补充矿石/锭的库存读数。
export default function install(api) {
  class SmelterFoundry extends api.builtins.OwnedStructure {
    get ore() {
      const store = api.view.field(this, 'store');
      return (store && store['smelter:ore']) || 0;
    }
    get ingots() {
      const store = api.view.field(this, 'store');
      return (store && store['smelter:ingot']) || 0;
    }
  }
  api.buildings.bind('smelter:foundry', SmelterFoundry);
  api.globals.define('SmelterFoundry', SmelterFoundry);
  api.globals.define('SMELTER_ORE', 'smelter:ore');
  api.globals.define('SMELTER_INGOT', 'smelter:ingot');
}
