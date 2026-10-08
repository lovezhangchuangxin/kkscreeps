// 玩家面绑定样本：安装 API 形态由 `kkscreeps mod types` 生成的声明
// 描述（TypedBeacon/TypedGate 构造器、FIND 常量、字段类型）；getter
// 读当轮已授权投影，字段级可见性由宿主裁切。
export default function install(api) {
  class TypedBeacon extends api.builtins.RoomObject {
    get level() {
      return api.view.field(this, 'level');
    }
    get state() {
      return api.view.field(this, 'state');
    }
    salute(amount, tag) {
      return api.intents.emit('alpha:salute', this, { amount, tag });
    }
  }
  class TypedGate extends api.builtins.Structure {}
  api.objects.bind('alpha:beacon', TypedBeacon);
  api.buildings.bind('alpha:gate', TypedGate);
  api.globals.define('TypedBeacon', TypedBeacon);
  api.globals.define('TypedGate', TypedGate);
  api.globals.define('FIND_TYPED_BEACONS', api.queries.install('alpha:beacon').find);
}
