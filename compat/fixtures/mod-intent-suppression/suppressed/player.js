export default function install(api) {
  const { RoomObject } = api.builtins;
  class Counter extends RoomObject {
    get value() { return api.view.field(this, 'value'); }
  }
  api.objects.bind('suppress:counter', Counter);
  const q = api.queries.install('suppress:counter');
  api.globals.define('FIND_COUNTERS', q.find);
}
