import { describe, expect, it } from 'vitest';
import {
  PLAYER_COLOR,
  PLAYER_COLOR_HEX,
  RAMPART_COVER,
  playerColor,
  playerColorHex,
  rampartCoverColor,
  SPAWN_BADGE_COLOR,
} from '../src/objects/player-color.js';

// 玩家色（G9：userOwnerColor 模式色板——demo 基准 worldConfigs.js:32）。

const isP1 = (u: string | null | undefined): boolean => u === 'p1';

describe('playerColor（_all.metadata.js:38-52 userOwnerColor 分支）', () => {
  it('own 绿 / enemy 红 / 未知灰', () => {
    expect(playerColor('p1', isP1)).toBe(PLAYER_COLOR.own);
    expect(playerColor('p2', isP1)).toBe(PLAYER_COLOR.enemy);
    expect(playerColor(null, isP1)).toBe(PLAYER_COLOR.unknown);
    expect(playerColor(undefined, isP1)).toBe(PLAYER_COLOR.unknown);
    expect(PLAYER_COLOR.own).toBe(0x8fbb93);
    expect(PLAYER_COLOR.enemy).toBe(0xff7777);
    expect(PLAYER_COLOR.unknown).toBe(0xbbbbbb);
  });

  it('hex 版本（文字面消费）', () => {
    expect(playerColorHex('p1', isP1)).toBe(PLAYER_COLOR_HEX.own);
    expect(playerColorHex('p2', isP1)).toBe(PLAYER_COLOR_HEX.enemy);
    expect(playerColorHex(null, isP1)).toBe(PLAYER_COLOR_HEX.unknown);
    expect(PLAYER_COLOR_HEX.own).toBe('#8fbb93');
    expect(PLAYER_COLOR_HEX.enemy).toBe('#FF7777');
    expect(PLAYER_COLOR_HEX.unknown).toBe('#ffffff');
  });
});

describe('rampart 顶罩色（terrain.js:205-214 userOwnerColor 分支）', () => {
  it('own #105010/#44ff44、enemy #501010/#ff4444', () => {
    expect(rampartCoverColor('p1', isP1)).toEqual(RAMPART_COVER.own);
    expect(rampartCoverColor('p2', isP1)).toEqual(RAMPART_COVER.enemy);
    expect(RAMPART_COVER.own).toEqual({ fill: 0x105010, stroke: 0x44ff44 });
    expect(RAMPART_COVER.enemy).toEqual({ fill: 0x501010, stroke: 0xff4444 });
  });

  it('user 缺位视作 enemy', () => {
    expect(rampartCoverColor(null, isP1)).toEqual(RAMPART_COVER.enemy);
  });
});

describe('spawn 徽标色（spawn.metadata.js:52-57 userOwnerColor 分支）', () => {
  it('恒 0x555555', () => {
    expect(SPAWN_BADGE_COLOR).toBe(0x555555);
  });
});
