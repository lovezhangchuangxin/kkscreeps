export default function install(api) {
  class VaultCache extends api.builtins.RoomObject {
    get hits() { return api.view.field(this, 'hits'); }
    get hitsMax() { return api.view.field(this, 'hitsMax'); }
    get store() { return api.view.field(this, 'store'); }
    get storeCapacity() { return api.view.field(this, 'storeCapacity'); }
  }
  api.objects.bind('objhits-vault:cache', VaultCache);
  api.globals.define('FIND_VAULT_CACHES', api.queries.install('objhits-vault:cache').find);
}
