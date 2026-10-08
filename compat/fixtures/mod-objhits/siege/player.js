export default function install(api) {
  class SiegeBarrel extends api.builtins.RoomObject {
    get hits() {
      return api.view.field(this, 'hits');
    }
    get hitsMax() {
      return api.view.field(this, 'hitsMax');
    }
  }
  class SiegeDecoy extends api.builtins.RoomObject {}
  api.objects.bind('objhits-siege:barrel', SiegeBarrel);
  api.objects.bind('objhits-siege:decoy', SiegeDecoy);
  api.globals.define('FIND_SIEGE_BARRELS', api.queries.install('objhits-siege:barrel').find);
  api.globals.define('FIND_SIEGE_DECOYS', api.queries.install('objhits-siege:decoy').find);
}
