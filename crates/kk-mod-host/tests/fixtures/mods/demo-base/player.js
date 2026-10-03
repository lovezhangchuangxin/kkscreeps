// 玩家面模块：与 demo-counter 的安装 API 同一形态，独立安装本包类型。
export default function install(api) {
  const { RoomObject } = api.builtins;
  class Widget extends RoomObject {
    get charge() { return api.view.field(this, 'charge'); }
    get owner() { return api.view.field(this, 'owner'); }
    recharge(amount = 1) {
      return api.intents.emit('demo-base:recharge', this, { amount });
    }
  }
  api.objects.bind('demo-base:widget', Widget);
  api.globals.define('Widget', Widget);
  const queries = api.queries.install('demo-base:widget');
  api.globals.define('FIND_WIDGETS', queries.find);
  api.globals.define('LOOK_WIDGETS', queries.look);
}
