export default function install(api) {
  class WardenAegis extends api.builtins.RoomObject {
    get hits() {
      return api.view.field(this, 'hits');
    }
    get hitsMax() {
      return api.view.field(this, 'hitsMax');
    }
  }
  class WardenDecoy extends api.builtins.RoomObject {}
  api.objects.bind('objguard-warden:aegis', WardenAegis);
  api.globals.define('FIND_WARDEN_AEGISES', api.queries.install('objguard-warden:aegis').find);
  api.objects.bind('objguard-warden:decoy', WardenDecoy);
  api.globals.define('FIND_WARDEN_DECOYS', api.queries.install('objguard-warden:decoy').find);
}
