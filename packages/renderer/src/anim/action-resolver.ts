/** Input normalization only: no Pixi, no state-change heuristics. */
import {
  EVENT_ATTACK_TYPE_MELEE, EVENT_ATTACK_TYPE_RANGED, EVENT_ATTACK_TYPE_RANGED_MASS,
  EVENT_HEAL_TYPE_MELEE, EVENT_HEAL_TYPE_RANGED,
  EVENT_ATTACK, EVENT_ATTACK_CONTROLLER, EVENT_BUILD, EVENT_HARVEST, EVENT_HEAL,
  EVENT_REPAIR, EVENT_RESERVE_CONTROLLER, EVENT_UPGRADE_CONTROLLER, EVENT_TRANSFER,
  type RoomEvent, type RoomObjectState, type SayEntry,
} from '@kkscreeps/shared';
import { resolveTypeKey } from '../objects/z-table.js';

export interface Point { readonly x: number; readonly y: number }
export type TargetAction = 'attack' | 'rangedAttack' | 'heal' | 'rangedHeal' | 'harvest'
  | 'build' | 'repair' | 'upgradeController' | 'reserveController' | 'attackController'
  | 'claimController' | 'transferEnergy' | 'power' | 'runReaction' | 'reverseReaction';
export interface TargetCommand {
  readonly action: TargetAction;
  readonly sourceId: string;
  readonly sourceType: string;
  readonly source: Point;
  readonly target: Point;
  readonly targetId?: string;
  readonly powerId?: number;
  readonly origin: 'event' | 'actionLog';
}
export type EffectCommand = TargetCommand
  | { readonly action: 'rangedMassAttack'; readonly sourceId: string; readonly source: Point; readonly origin: 'event' | 'actionLog' }
  | { readonly action: 'say'; readonly sourceId: string; readonly message: string; readonly private: boolean; readonly origin: 'say' | 'actionLog' };
export interface Hit { readonly targetId: string; readonly attacked: boolean; readonly healed: boolean }
export interface Resolution { readonly commands: EffectCommand[]; readonly hits: Hit[]; readonly skipped: number }
export interface ActionInput {
  readonly current: ReadonlyMap<string, RoomObjectState>;
  readonly previous: ReadonlyMap<string, RoomObjectState>;
  readonly events?: readonly RoomEvent[];
  readonly actionLogs?: readonly unknown[];
  readonly says?: readonly SayEntry[];
}

function record(v: unknown): Record<string, unknown> | null {
  return typeof v === 'object' && v !== null && !Array.isArray(v) ? v as Record<string, unknown> : null;
}
export function point(v: unknown): Point | null {
  const r = record(v);
  return r && typeof r.x === 'number' && typeof r.y === 'number'
    && Number.isFinite(r.x) && Number.isFinite(r.y) && r.x >= 0 && r.x <= 49 && r.y >= 0 && r.y <= 49
    ? { x: r.x, y: r.y } : null;
}
const TARGET_ACTIONS: readonly TargetAction[] = [
  'attack', 'rangedAttack', 'heal', 'rangedHeal', 'harvest', 'build', 'repair',
  'upgradeController', 'reserveController', 'attackController', 'claimController', 'transferEnergy', 'power',
];

export function resolveActions(input: ActionInput): Resolution {
  const commands = new Map<string, EffectCommand>();
  const hits = new Map<string, { targetId: string; attacked: boolean; healed: boolean }>();
  let skipped = 0;
  const hit = (id: string, attack: boolean, heal: boolean) => {
    const old = hits.get(id) ?? { targetId: id, attacked: false, healed: false };
    old.attacked ||= attack; old.healed ||= heal; hits.set(id, old);
  };
  const add = (c: EffectCommand) => {
    // Coordinate semantics also match action logs that have no targetId.
    const target = 'target' in c ? c.targetId ? `id:${c.targetId}` : `xy:${c.target.x},${c.target.y}` : '';
    const source = 'source' in c ? `${c.source.x},${c.source.y}` : '';
    let key = `${c.sourceId}:${c.action}:${target}:${c.action === 'runReaction' || c.action === 'reverseReaction' ? source : ''}`;
    if ('target' in c && c.action !== 'runReaction' && c.action !== 'reverseReaction') {
      for (const [oldKey, old] of commands) {
        if (!('target' in old) || old.sourceId !== c.sourceId || old.action !== c.action) continue;
        const same = c.targetId && old.targetId ? c.targetId === old.targetId : old.target.x === c.target.x && old.target.y === c.target.y;
        if (same) { key = oldKey; break; }
      }
    }
    commands.set(key, c); // Events first; explicit actionLog wins.
  };
  const controller = [...input.current.values()].filter(s => resolveTypeKey(s) === 's:controller');
  const targetFor = (data: Record<string, unknown>, useController: boolean) => {
    const id = typeof data.targetId === 'string' ? data.targetId : useController && controller.length === 1 ? controller[0]!.id : undefined;
    const target = point(data) ?? (id ? point(input.current.get(id)) ?? point(input.previous.get(id)) : null);
    return target ? { target, targetId: id } : null;
  };
  for (const event of input.events ?? []) {
    const data = record(event.data) ?? {};
    const id = typeof data.targetId === 'string' ? data.targetId : null;
    if (id && (event.event === EVENT_ATTACK || event.event === EVENT_HEAL)) {
      hit(id, event.event === EVENT_ATTACK, event.event === EVENT_HEAL);
    }
    let action: TargetAction | 'rangedMassAttack' | undefined;
    switch (event.event) {
      case EVENT_ATTACK:
        if (data.attackType === EVENT_ATTACK_TYPE_MELEE) action = 'attack';
        else if (data.attackType === EVENT_ATTACK_TYPE_RANGED) action = 'rangedAttack';
        else if (data.attackType === EVENT_ATTACK_TYPE_RANGED_MASS) action = 'rangedMassAttack';
        break; // Dismantle/hit-back/nuke are damage, not a melee animation.
      case EVENT_HEAL:
        if (data.healType === EVENT_HEAL_TYPE_MELEE) action = 'heal';
        else if (data.healType === EVENT_HEAL_TYPE_RANGED) action = 'rangedHeal';
        break;
      case EVENT_BUILD: action = 'build'; break;
      case EVENT_HARVEST: action = 'harvest'; break;
      case EVENT_REPAIR: action = 'repair'; break;
      case EVENT_UPGRADE_CONTROLLER: action = 'upgradeController'; break;
      case EVENT_RESERVE_CONTROLLER: action = 'reserveController'; break;
      case EVENT_ATTACK_CONTROLLER: action = 'attackController'; break;
      case EVENT_TRANSFER:
        if (data.resourceType === 'energy' && resolveTypeKey(input.current.get(event.objectId) ?? input.previous.get(event.objectId) ?? { id: '', type: '' }) === 's:link'
          && id && resolveTypeKey(input.current.get(id) ?? input.previous.get(id) ?? { id: '', type: '' }) === 's:link') action = 'transferEnergy';
        break;
    }
    if (!action) continue;
    const state = input.current.get(event.objectId) ?? input.previous.get(event.objectId);
    const source = point(state);
    if (!state || !source) { skipped++; continue; }
    if (action === 'rangedMassAttack') { add({ action, sourceId: state.id, source, origin: 'event' }); continue; }
    const resolved = targetFor(data, action.endsWith('Controller'));
    if (!resolved) { skipped++; continue; }
    add({ action, sourceId: state.id, sourceType: resolveTypeKey(state), source, ...resolved, origin: 'event' });
  }
  for (const raw of input.actionLogs ?? []) {
    const entry = record(raw);
    const log = record(entry?.actionLog);
    if (typeof entry?.id !== 'string' || !log) { skipped++; continue; }
    const state = input.current.get(entry.id) ?? input.previous.get(entry.id);
    const source = point(state);
    if (!state || !source) { skipped++; continue; }
    if (log.attacked === true || log.healed === true) hit(state.id, log.attacked === true, log.healed === true);
    for (const action of TARGET_ACTIONS) {
      if (log[action] === undefined) continue;
      const data = record(log[action]);
      const resolved = data && targetFor(data, false);
      if (!resolved) { skipped++; continue; }
      add({ action, sourceId: state.id, sourceType: resolveTypeKey(state), source, ...resolved,
        powerId: action === 'power' && typeof data?.powerId === 'number' ? data.powerId : undefined, origin: 'actionLog' });
    }
    if (log.rangedMassAttack === true) add({ action: 'rangedMassAttack', sourceId: state.id, source, origin: 'actionLog' });
    for (const action of ['runReaction', 'reverseReaction'] as const) {
      if (log[action] === undefined) continue;
      const r = record(log[action]);
      const a = point({ x: r?.x1, y: r?.y1 });
      const b = point({ x: r?.x2, y: r?.y2 });
      if (!a || !b) { skipped++; continue; }
      for (const p of [a, b]) add({ action, sourceId: state.id, sourceType: resolveTypeKey(state),
        source: action === 'runReaction' ? p : source, target: action === 'runReaction' ? source : p, origin: 'actionLog' });
    }
    const say = record(log.say);
    if (say && typeof say.message === 'string' && typeof say.isPublic === 'boolean') {
      add({ action: 'say', sourceId: state.id, message: say.message, private: !say.isPublic, origin: 'actionLog' });
    }
  }
  for (const say of input.says ?? []) {
    if (typeof say?.id !== 'string' || typeof say.message !== 'string' || typeof say.private !== 'boolean' || !input.current.has(say.id)) { skipped++; continue; }
    add({ action: 'say', sourceId: say.id, message: say.message, private: say.private, origin: 'say' });
  }
  return { commands: [...commands.values()], hits: [...hits.values()], skipped };
}
