import { describe, expect, it } from 'vitest';
import { Z_TABLE, resolveTypeKey } from '../src/objects/z-table.js';
import { PART_DECL_TABLE, assertUniquePartIds } from '../src/objects/parts-table.js';

// z 序全表（〔官拆〕§2 权威值逐项）与类型键推导。

describe('Z_TABLE（官方 zIndex 全表）', () => {
  it('与官方 metadata 顶层 zIndex 逐项一致', () => {
    expect(Z_TABLE['s:road']).toBe(-1); // road.metadata.js:12
    expect(Z_TABLE['s:rampart']).toBe(0); // rampart.metadata.js:25
    expect(Z_TABLE['s:constructedWall']).toBe(0); // constructedWall.metadata.js:38
    expect(Z_TABLE['s:extractor']).toBe(0); // extractor.metadata.js:33
    expect(Z_TABLE.flag).toBe(0); // flag.metadata.js 无 zIndex → ||0
    expect(Z_TABLE.resource).toBe(1); // energy.metadata.js:19
    expect(Z_TABLE['s:deposit']).toBe(1); // deposit.metadata.js:124
    expect(Z_TABLE['s:source']).toBe(2); // source.metadata.js:175
    expect(Z_TABLE['s:mineral']).toBe(2); // mineral.metadata.js:121
    expect(Z_TABLE['s:portal']).toBe(3); // portal.metadata.js:187
    expect(Z_TABLE['s:keeperLair']).toBe(3); // keeperLair.metadata.js:118
    expect(Z_TABLE['s:controller']).toBe(4); // controller.metadata.js:466
    expect(Z_TABLE['s:container']).toBe(4); // container.metadata.js:109
    expect(Z_TABLE.tombstone).toBe(5); // tombstone.metadata.js:70
    expect(Z_TABLE.ruin).toBe(5); // ruin.metadata.js:70
    expect(Z_TABLE.creep).toBe(6); // creep.metadata.js
    expect(Z_TABLE['s:extension']).toBe(7); // extension.metadata.js:159
    expect(Z_TABLE['s:factory']).toBe(7); // factory.metadata.js:241
    expect(Z_TABLE['s:storage']).toBe(7); // storage.metadata.js:128
    expect(Z_TABLE['s:nuker']).toBe(7); // nuker.metadata.js:125
    expect(Z_TABLE.spawn).toBe(8); // spawn.metadata.js:270
    expect(Z_TABLE['s:link']).toBe(9); // link.metadata.js:100
    expect(Z_TABLE['s:observer']).toBe(10); // observer.metadata.js:128
    expect(Z_TABLE['s:powerBank']).toBe(11); // powerBank.metadata.js:132
    expect(Z_TABLE['s:powerSpawn']).toBe(12); // powerSpawn.metadata.js:267
    expect(Z_TABLE['s:tower']).toBe(13); // tower.metadata.js:250
    expect(Z_TABLE.powerCreep).toBe(13); // 〔官拆〕§2
    expect(Z_TABLE['s:lab']).toBe(15); // lab.metadata.js:236（z=14 无主）
    expect(Z_TABLE['s:terminal']).toBe(16); // terminal.metadata.js:183
    expect(Z_TABLE['s:invaderCore']).toBe(17); // invaderCore.metadata.js:102
    expect(Z_TABLE.constructionSite).toBe(18); // constructionSite.metadata.js:26
  });
});

describe('resolveTypeKey', () => {
  it('structure + structureType → s:<type>', () => {
    expect(resolveTypeKey({ id: '1', type: 'structure', structureType: 'tower' })).toBe('s:tower');
    expect(resolveTypeKey({ id: '1', type: 'structure', structureType: 'constructedWall' }))
      .toBe('s:constructedWall');
  });
  it('spawn/constructionSite/resource 自成键（structureType 单向蕴含）', () => {
    expect(resolveTypeKey({ id: '1', type: 'spawn', structureType: 'spawn' })).toBe('spawn');
    expect(resolveTypeKey({ id: '1', type: 'constructionSite', structureType: 'extension' }))
      .toBe('constructionSite');
    expect(resolveTypeKey({ id: '1', type: 'resource', resourceType: 'energy' })).toBe('resource');
  });
  it('其余类型直通', () => {
    expect(resolveTypeKey({ id: '1', type: 'creep' })).toBe('creep');
    expect(resolveTypeKey({ id: '1', type: 'tombstone' })).toBe('tombstone');
    expect(resolveTypeKey({ id: '1', type: 'ruin' })).toBe('ruin');
    expect(resolveTypeKey({ id: '1', type: 'flag', color: 1 })).toBe('flag');
  });
});

describe('PART_DECL_TABLE', () => {
  it('键集 = Z_TABLE 键集 − road/creep/powerCreep', () => {
    const expected = new Set(Object.keys(Z_TABLE));
    expected.delete('s:road');
    expected.delete('creep');
    expected.delete('powerCreep');
    expect(new Set(Object.keys(PART_DECL_TABLE))).toEqual(expected);
  });

  it('每个类型的部件 id 唯一', () => {
    for (const [typeKey, parts] of Object.entries(PART_DECL_TABLE)) {
      expect(() => assertUniquePartIds(parts, typeKey)).not.toThrow();
    }
  });

  it('含 store/user 消费面的类型追加 _isDisabled 公共件', () => {
    for (const typeKey of ['s:extension', 's:storage', 's:factory', 's:nuker', 'spawn', 's:link', 's:observer', 's:tower', 's:lab', 's:terminal', 's:extractor', 's:controller', 's:container']) {
      const ids = PART_DECL_TABLE[typeKey]!.map((p) => p.id);
      expect(ids, typeKey).toContain('disabled');
    }
  });
});
