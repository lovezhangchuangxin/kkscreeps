import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CanvasTextMetrics } from 'pixi.js';
import type { RoomVisual } from '@kkscreeps/shared';
import { VisualStage } from '../src/scenes/visual-stage.js';

// pixi 半件冒烟：重建/引用短路/无效条目降级/Text 精确销毁路径。
// node 无 canvas——CanvasTextMetrics 桩（R5 say 气泡测试同款）。

function measureStub(): void {
  vi.spyOn(CanvasTextMetrics, 'measureText').mockImplementation(() => ({ width: 180 }) as CanvasTextMetrics);
}

let stage: VisualStage;
beforeEach(() => {
  measureStub();
  stage = new VisualStage(100);
});
afterEach(() => {
  stage.destroy();
  vi.mocked(CanvasTextMetrics.measureText).mockRestore();
});

const visuals: RoomVisual[] = [
  { type: 'line', points: [{ x: 1, y: 1 }, { x: 5, y: 1 }] },
  { type: 'rect', x: 2, y: 2, w: 3, h: 2, style: { fill: 'transparent', stroke: '#ffffff' } },
  { type: 'circle', x: 10, y: 10, style: { radius: 2, fill: '#ff0000' } },
  { type: 'poly', points: [{ x: 0, y: 0 }, { x: 4, y: 0 }, { x: 2, y: 3 }], style: { fill: '#00ff0011' } },
  { type: 'text', x: 5, y: 5, text: '标注', style: { backgroundColor: '#000000', backgroundPadding: 1 } },
];

describe('VisualStage', () => {
  it('五原语全渲染，count=条数，root 挂载', () => {
    stage.setVisuals(visuals);
    expect(stage.count).toBe(5);
    expect(stage.root.children.length).toBe(5);
  });

  it('同引用零成本短路；新引用全量重建（节点重建不累积）', () => {
    stage.setVisuals(visuals);
    const firstBefore = stage.root.children[0];
    stage.setVisuals(visuals);
    expect(stage.root.children[0]).toBe(firstBefore); // 引用相同 → 不重建
    stage.setVisuals([...visuals]);
    expect(stage.root.children[0]).not.toBe(firstBefore); // 新引用 → 重建
    expect(stage.root.children.length).toBe(5);
  });

  it('全量替换：数量减少后节点同步收缩', () => {
    stage.setVisuals(visuals);
    stage.setVisuals([visuals[0]!]);
    expect(stage.count).toBe(1);
    expect(stage.root.children.length).toBe(1);
    stage.setVisuals([]);
    expect(stage.count).toBe(0);
    expect(stage.root.children.length).toBe(0);
  });

  it('无效条目降级跳过不崩溃（guard 拦截；NaN 半径走防御默认渲染）', () => {
    const dirty = [
      { type: 'circle', x: 1, y: 1, style: { radius: Number.NaN } }, // 结构有效→防御默认半径渲染
      { type: 'text', x: 2, y: 2, text: 'ok' },
      { type: 'banana', x: 0, y: 0 }, // guard 拦截
      { type: 'line', points: [{ x: 'a' }] }, // guard 拦截
    ] as unknown as RoomVisual[];
    expect(() => stage.setVisuals(dirty)).not.toThrow();
    expect(stage.count).toBe(2);
    expect(stage.root.children.length).toBe(2);
  });

  it('text 条目带背景底衬与文本两层', () => {
    stage.setVisuals([{ type: 'text', x: 5, y: 5, text: 'x', style: { backgroundColor: '#000000' } }]);
    const container = stage.root.children[0]!;
    expect(container.children.length).toBe(2); // bg Graphics + Text
  });

  it('destroy 清空子树（可重复）', () => {
    stage.setVisuals(visuals);
    stage.destroy();
    expect(stage.count).toBe(0);
    expect(stage.root.destroyed).toBe(true);
  });
});

describe('几何换算锚点（格中心相位，pixel=游戏坐标×cell）', () => {
  it('circle 路径进像素域（中心=格中心，半径×cell）', () => {
    const s = new VisualStage(100);
    try {
      s.setVisuals([{ type: 'circle', x: 3, y: 4, style: { radius: 1.5 } }]);
      const b = s.root.children[0]!.getBounds();
      expect(b.x).toBeCloseTo(150);
      expect(b.y).toBeCloseTo(250);
      expect(b.width).toBeCloseTo(300);
      expect(b.height).toBeCloseTo(300);
    } finally {
      s.destroy();
    }
  });
  it('text 容器位置=格中心像素', () => {
    stage.setVisuals([{ type: 'text', x: 3, y: 4, text: 'p' }]);
    const container = stage.root.children[0]!;
    expect(container.x).toBe(300);
    expect(container.y).toBe(400);
  });
});

describe('重建不含 pixi 树泄漏（重复 setVisuals 后 children 恰等于条数）', () => {
  it('50 轮替换后节点数稳定', () => {
    for (let i = 0; i < 50; i++) {
      stage.setVisuals(i % 2 === 0 ? visuals : [visuals[1]!]);
      expect(stage.root.children.length).toBe(i % 2 === 0 ? 5 : 1);
    }
  });
});
