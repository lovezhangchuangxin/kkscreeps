export default function install(api) {
  class Beacon extends api.builtins.OwnedStructure {}
  api.buildings.bind('bk:beacon', Beacon);
}
