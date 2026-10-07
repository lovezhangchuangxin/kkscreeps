export default function install(api) {
  class VaultCache extends api.builtins.RoomObject {
    get store() { return api.view.field(this, 'store'); }
    get storeCapacity() { return api.view.field(this, 'storeCapacity'); }
  }
  api.objects.bind('objstore-vault:cache', VaultCache);
  api.globals.define('FIND_VAULT_CACHES', api.queries.install('objstore-vault:cache').find);
}
