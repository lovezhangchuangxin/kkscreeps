// 用生产对象工厂建立同一成熟房；不伪造通用结构绕过 schema/processor。
import { createRoomObject } from 'xxscreeps/game/object.js';
import { create as extension } from 'xxscreeps/mods/classic/spawn/extension.js';
import { create as storage } from 'xxscreeps/mods/classic/logistics/storage.js';
import { create as link } from 'xxscreeps/mods/classic/logistics/link.js';
import { create as tower } from 'xxscreeps/mods/classic/defense/tower.js';
import { create as rampart } from 'xxscreeps/mods/classic/defense/rampart.js';
import { create as wall } from 'xxscreeps/mods/classic/defense/wall.js';
import { create as road } from 'xxscreeps/mods/classic/road/road.js';
import { create as container } from 'xxscreeps/mods/classic/resource/container.js';
import { create as terminal } from 'xxscreeps/mods/classic/brokerage/terminal.js';
import { create as lab } from 'xxscreeps/mods/classic/chemistry/lab.js';
import { create as extractor } from 'xxscreeps/mods/classic/mineral/extractor.js';
import { Mineral } from 'xxscreeps/mods/classic/mineral/mineral.js';
import { create as factory } from 'xxscreeps/mods/modern/factory/factory.js';
import { create as observer } from 'xxscreeps/mods/modern/observer/observer.js';
import { create as powerSpawn } from 'xxscreeps/mods/modern/powerspawn/powerspawn.js';
import { create as nuker } from 'xxscreeps/mods/modern/nuker/nuker.js';

const creators = {
  storage,
  link,
  tower,
  rampart,
  constructedWall: wall,
  road,
  container,
  terminal,
  lab,
  extractor,
  factory,
  observer,
  powerSpawn,
  nuker,
};
export function create(kind, spec, pos) {
  let object;
  if (kind === 'mineral') {
    object = createRoomObject(new Mineral(), pos);
    object.mineralType = spec.mineralType;
    object.mineralAmount = spec.mineralAmount;
    object.density = spec.density;
  } else if (spec.structureType === 'extension') object = extension(pos, 8, spec.owner);
  else {
    const creator = creators[spec.structureType];
    if (!creator) throw new Error('Unsupported mature fixture object ' + spec.structureType);
    object = creator(pos, spec.owner);
  }
  // kk 管理放置给完整衰减窗口；xxs 工厂原用于 nextTime 的结算期，管理读面装载补一拍。
  if (['road', 'rampart', 'container'].includes(spec.structureType)) object['#nextDecayTime']++;
  if (spec.hits !== undefined) object.hits = spec.hits;
  return object;
}
export function fill(object, spec) {
  if (object.store) {
    // spawn 工厂自带初始能量；先清零再按共享夹具赋值，不能累加默认库存。
    for (const resource of Object.keys(object.store))
      object.store['#subtract'](resource, object.store[resource]);
    for (const [resource, amount] of Object.entries(spec.store || {}))
      object.store['#add'](resource, amount);
  }
}
const store = (object) =>
  Object.fromEntries(
    Object.keys(object.store || {})
      .filter((key) => object.store[key] > 0)
      .sort()
      .map((key) => [key, object.store[key]]),
  );
export function observe(room) {
  const objects = room['#objects'];
  return {
    room: room.name,
    level: room.controller.level,
    structures: objects
      .filter((o) => o.structureType && o.structureType !== 'controller')
      .map((o) => ({
        type: o.structureType,
        x: o.pos.x,
        y: o.pos.y,
        hits: o.hits,
        store: store(o),
        cooldown: o.cooldown || 0,
        spawning: !!o.spawning,
      }))
      .sort((a, b) => (a.type < b.type ? -1 : a.type > b.type ? 1 : a.y - b.y || a.x - b.x)),
    creeps: objects
      .filter((o) => o.constructor.name === 'Creep')
      .map((o) => ({
        name: o.name,
        x: o.pos.x,
        y: o.pos.y,
        hits: o.hits,
        store: store(o),
        fatigue: o.fatigue,
        ttl: o.ticksToLive ?? null,
        spawning: o.spawning,
        body: o.body.map((p) => p.type),
      }))
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)),
    sources: objects
      .filter((o) => o.constructor.name === 'Source')
      .map((o) => ({ x: o.pos.x, y: o.pos.y, energy: o.energy, capacity: o.energyCapacity }))
      .sort((a, b) => a.y - b.y || a.x - b.x),
    minerals: objects
      .filter((o) => o instanceof Mineral)
      .map((o) => ({ x: o.pos.x, y: o.pos.y, type: o.mineralType, amount: o.mineralAmount }))
      .sort((a, b) => a.y - b.y || a.x - b.x),
  };
}
