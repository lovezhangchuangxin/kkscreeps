export default function install(api) {
  class ThornSpike extends api.builtins.RoomObject {
    get hits() { return api.view.field(this, 'hits'); }
    get hitsMax() { return api.view.field(this, 'hitsMax'); }
  }
  api.objects.bind('objret-thorn:spike', ThornSpike);
  api.globals.define('FIND_THORN_SPIKES', api.queries.install('objret-thorn:spike').find);
}
