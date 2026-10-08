export default function install(api) {
  class RulesMeter extends api.builtins.RoomObject {
    get value() {
      return api.view.field(this, 'value');
    }
    get pool() {
      return api.view.field(this, 'pool');
    }
  }
  api.objects.bind('base:meter', RulesMeter);
  api.globals.define('RulesMeter', RulesMeter);
  api.globals.define('FIND_RULES_METERS', api.queries.install('base:meter').find);
}
