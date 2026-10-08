export default function install(api) {
  class HaulerCrate extends api.builtins.RoomObject {
    get store() {
      return api.view.field(this, 'store');
    }
    get storeCapacity() {
      return api.view.field(this, 'storeCapacity');
    }
  }
  class HaulerMarker extends api.builtins.RoomObject {}
  api.objects.bind('objstore-hauler:crate', HaulerCrate);
  api.objects.bind('objstore-hauler:marker', HaulerMarker);
  api.globals.define('FIND_HAULER_CRATES', api.queries.install('objstore-hauler:crate').find);
  api.globals.define('FIND_HAULER_MARKERS', api.queries.install('objstore-hauler:marker').find);
}
