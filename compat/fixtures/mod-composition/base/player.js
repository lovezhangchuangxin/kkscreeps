export default function install(api) {
  class CompositionCounter extends api.builtins.RoomObject {
    get value() {
      return api.view.field(this, 'value');
    }
    get trace() {
      return api.view.field(this, 'trace');
    }
  }
  api.objects.bind('base:counter', CompositionCounter);
  api.globals.define('CompositionCounter', CompositionCounter);
  api.globals.define('FIND_COMPOSITION_COUNTERS', api.queries.install('base:counter').find);
}
