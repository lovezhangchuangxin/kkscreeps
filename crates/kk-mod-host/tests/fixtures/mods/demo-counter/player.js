// 玩家面模块：在每个玩家 VM 内执行（宿主以 install(api) 调用本导出）。
// 类与 getter 在玩家 VM 内生成；字段值经 api.view 从当轮已授权投影读取，
// 不携带服务器私有数据。
export default function install(api) {
  const { RoomObject } = api.builtins;
  class Counter extends RoomObject {
    get value() {
      return api.view.field(this, 'value');
    }
    get owner() {
      return api.view.field(this, 'owner');
    }
    increment(amount = 1) {
      return api.intents.emit('demo-counter:increment', this, { amount });
    }
  }
  api.objects.bind('demo-counter:counter', Counter);
  api.globals.define('Counter', Counter);
  const queries = api.queries.install('demo-counter:counter');
  api.globals.define('FIND_COUNTERS', queries.find);
  api.globals.define('LOOK_COUNTERS', queries.look);
}
