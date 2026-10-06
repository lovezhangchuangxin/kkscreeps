// 玩家类糖：宿主直供存储键（含冒号），类上补语义命名。
export default function install(api) {
  class Gauge extends api.builtins.OwnedStructure {
    get heat() { return this['bl:heat']; }
  }
  api.buildings.bind('bl:gauge', Gauge);
}
