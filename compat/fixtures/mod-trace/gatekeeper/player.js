// 玩家面绑定：门对象可查询（FIND 常量）并可经类方法发出 pass/hold
// 意图；hold 的返回码是 OK/ERR_INVALID_ARGS（预检通过即记录，拒绝由
// 结算侧在链内表达——trace 的 slot_declined 事件即此形态）。
export default function install(api) {
  class GatekeeperDoor extends api.builtins.RoomObject {
    get value() {
      return api.view.field(this, 'value');
    }
    pass() {
      return api.intents.emit('gatekeeper:pass', this, {});
    }
    hold() {
      return api.intents.emit('gatekeeper:hold', this, {});
    }
  }
  api.objects.bind('gatekeeper:door', GatekeeperDoor);
  api.globals.define('GatekeeperDoor', GatekeeperDoor);
  api.globals.define('FIND_GATEKEEPER_DOORS', api.queries.install('gatekeeper:door').find);
}
