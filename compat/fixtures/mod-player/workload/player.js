export default function install(api) {
  class Counter extends api.builtins.RoomObject {
    get value() { return api.view.field(this, 'value'); }
    increment() { return api.intents.emit('workload:increment', this, {amount: 1}); }
  }
  api.objects.bind('workload:counter', Counter);
  api.globals.define('Counter', Counter);
  const queries = api.queries.install('workload:counter');
  api.globals.define('FIND_COUNTERS', queries.find);
  api.globals.define('LOOK_COUNTERS', queries.look);
}
