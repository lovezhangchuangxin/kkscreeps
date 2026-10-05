export default function install(api) {
  class PanelCollector extends api.builtins.OwnedStructure {
    get charge() {
      const store = api.view.field(this, 'store');
      return (store && store.energy) || 0;
    }
  }
  api.buildings.bind('panel:collector', PanelCollector);
  api.globals.define('PanelCollector', PanelCollector);
}
