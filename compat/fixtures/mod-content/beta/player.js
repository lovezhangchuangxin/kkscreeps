export default function install(api) {
  class BetaBeacon extends api.builtins.RoomObject {
    get marks() { return api.view.field(this, 'marks'); }
  }
  api.objects.bind('beta:beacon', BetaBeacon);
  api.globals.define('BetaBeacon', BetaBeacon);
  api.globals.define('FIND_BETA_BEACONS', api.queries.install('beta:beacon').find);
}
