export default function install(api) {
  class ClockworkCounter extends api.builtins.RoomObject {
    get value() { return api.view.field(this, 'value'); }
  }
  api.objects.bind('clockwork:counter', ClockworkCounter);
  api.globals.define('ClockworkCounter', ClockworkCounter);
  const queries = api.queries.install('clockwork:counter');
  api.globals.define('FIND_CLOCKWORK_COUNTERS', queries.find);
  api.globals.define('LOOK_CLOCKWORK_COUNTERS', queries.look);
}
