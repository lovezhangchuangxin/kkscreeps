export default function install(api) {
  class AlphaNode extends api.builtins.RoomObject {
    get value() { return api.view.field(this, 'value'); }
  }
  api.objects.bind('alpha:node', AlphaNode);
  api.globals.define('AlphaNode', AlphaNode);
  api.globals.define('FIND_ALPHA_NODES', api.queries.install('alpha:node').find);
}
