// 玩家面绑定样本：归属者集合可见对象只对归属者出现在投影中，他人
// 查询不到实例；可空参数意图 emit 允许省略 args（宿主归一为 null）。
export default function install(api) {
  class TypedRelay extends api.builtins.RoomObject {
    get charge() { return api.view.field(this, 'charge'); }
    ping(code) {
      return api.intents.emit('beta:ping', this, code === undefined ? null : { code });
    }
  }
  api.objects.bind('beta:relay', TypedRelay);
  api.globals.define('TypedRelay', TypedRelay);
  api.globals.define('FIND_TYPED_RELAYS', api.queries.install('beta:relay').find);
}
