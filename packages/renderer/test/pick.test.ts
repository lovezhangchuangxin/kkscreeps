import { describe, expect, it } from 'vitest';
import { resolvePickObject } from '../src/feed/pick.js';

// 数据面命中（R6 D4）：z 序优先级 + id 码点平手；数据格命中语义。

describe('resolvePickObject', () => {
  it('空格 undefined', () => {
    expect(resolvePickObject({ x: 5, y: 5 }, [])).toBeUndefined();
  });

  it('同格多对象取 z 序高者（Z_TABLE：tower 13 > creep 6 > source 2）', () => {
    const objects = [
      { id: 'src', type: 'structure', structureType: 'source', x: 5, y: 5 },
      { id: 'creep', type: 'creep', x: 5, y: 5 },
      { id: 'tower', type: 'structure', structureType: 'tower', x: 5, y: 5 },
    ];
    expect(resolvePickObject({ x: 5, y: 5 }, objects)).toBe('tower');
  });

  it('z 平手取 id 码点序（确定性）', () => {
    const objects = [
      { id: 'b', type: 'creep', x: 1, y: 1 },
      { id: 'a', type: 'creep', x: 1, y: 1 },
    ];
    expect(resolvePickObject({ x: 1, y: 1 }, objects)).toBe('a');
  });

  it('只命中同格对象（邻格不算）', () => {
    const objects = [{ id: 'near', type: 'creep', x: 6, y: 5 }];
    expect(resolvePickObject({ x: 5, y: 5 }, objects)).toBeUndefined();
  });

  it('未知类型键 z 缺省 0 仍可命中', () => {
    const objects = [{ id: 'x', type: 'mystery', x: 0, y: 0 }];
    expect(resolvePickObject({ x: 0, y: 0 }, objects)).toBe('x');
  });
});
