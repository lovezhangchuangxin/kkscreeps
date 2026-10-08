export default function install(api) {
  const { RoomObject } = api.builtins;
  class FreeCounter extends RoomObject {
    get value() {
      return api.view.field(this, 'value');
    }
  }
  api.objects.bind('free:counter', FreeCounter);
  const q = api.queries.install('free:counter');
  // 查询全局用独立名：与 suppress 包组合选择时不撞全局。
  api.globals.define('FIND_FREE_COUNTERS', q.find);
}
