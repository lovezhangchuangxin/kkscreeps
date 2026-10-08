export default function install(api) {
  class NurseryEmitter extends api.builtins.RoomObject {
    get emitted() {
      return api.view.field(this, 'emitted');
    }
  }
  class NurserySpark extends api.builtins.RoomObject {
    get age() {
      return api.view.field(this, 'age');
    }
    get life() {
      return api.view.field(this, 'life');
    }
  }
  for (const [key, cls, label] of [
    ['nursery:emitter', NurseryEmitter, 'NURSERY_EMITTERS'],
    ['nursery:spark', NurserySpark, 'NURSERY_SPARKS'],
  ]) {
    api.objects.bind(key, cls);
    api.globals.define(cls.name, cls);
    const queries = api.queries.install(key);
    api.globals.define('FIND_' + label, queries.find);
    api.globals.define('LOOK_' + label, queries.look);
  }
}
