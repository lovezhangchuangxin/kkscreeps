export default function install(api) {
  class ReactorCore extends api.builtins.RoomObject {
    get hits() {
      return api.view.field(this, 'hits');
    }
    get hitsMax() {
      return api.view.field(this, 'hitsMax');
    }
    get store() {
      return api.view.field(this, 'store');
    }
    get storeCapacity() {
      return api.view.field(this, 'storeCapacity');
    }
  }
  api.objects.bind('objret-reactor:core', ReactorCore);
  api.globals.define('FIND_REACTOR_CORES', api.queries.install('objret-reactor:core').find);
}
