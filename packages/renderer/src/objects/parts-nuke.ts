import type { PartDecl, PartValues } from './part-decl.js';

export const PARTS_NUKE: PartDecl[] = ['marker', 'circle', 'pulse', 'glow'].map(id => ({
  id, keys: ['landTime'], values: (s, env): PartValues | null => typeof s.landTime === 'number' && s.landTime > env.time ? {} : null,
}));
