export default function install(api) {
  class LogbookMonolith extends api.builtins.RoomObject {
    get hits() { return api.view.field(this, 'hits'); }
    get hitsMax() { return api.view.field(this, 'hitsMax'); }
  }
  api.objects.bind('objguard-logbook:monolith', LogbookMonolith);
  api.globals.define('FIND_LOGBOOK_MONOLITHS', api.queries.install('objguard-logbook:monolith').find);
}
