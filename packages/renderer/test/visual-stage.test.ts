import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CanvasTextMetrics, Graphics, Text } from 'pixi.js';
import type { RoomVisual } from '@kkscreeps/shared';
import { VisualStage } from '../src/scenes/visual-stage.js';

// pixi 半件冒烟：重建/引用短路/无效条目降级/Text 精确销毁路径。
// node 无 canvas——CanvasTextMetrics 桩（R5 say 气泡测试同款）。

function measureStub(): void {
  vi.spyOn(CanvasTextMetrics, 'measureText').mockImplementation(() => ({ width: 180, height: 100 }) as CanvasTextMetrics);
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

  it('同引用短路；新引用复用同类节点且不累积', () => {
    stage.setVisuals(visuals);
    const firstBefore = stage.root.children[0];
    stage.setVisuals(visuals);
    expect(stage.root.children[0]).toBe(firstBefore); // 引用相同 → 不重建
    stage.setVisuals([...visuals]);
    expect(stage.root.children[0]).toBe(firstBefore); // 新引用 → 全量重绘，节点复用
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

  it('替换与销毁释放五原语和文字背景的真实 Graphics context', () => {
    stage.setVisuals(visuals);
    const contexts = stage.root.children.flatMap(node => node instanceof Graphics
      ? [node.context] : node.children.filter((c): c is Graphics => c instanceof Graphics).map(c => c.context));
    expect(contexts).toHaveLength(5);
    stage.setVisuals([visuals[0]!]);
    expect(contexts[0]!.destroyed).toBe(false);
    expect(contexts.slice(1).every(context => context.destroyed)).toBe(true);
    const last = (stage.root.children[0] as Graphics).context;
    stage.destroy(); expect(last.destroyed).toBe(true);
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

  it('透明填充保留独立描边 alpha，背景 middle 几何对称', () => {
    stage.setVisuals([{ type: 'text', x: 1, y: 1, text: 'x', style: {
      color: 'transparent', stroke: '#ff000080', backgroundColor: '#00000080', opacity: 0.5,
    } }]);
    const container = stage.root.children[0]!;
    const text = container.children[1] as Text;
    expect(text.alpha).toBe(0.5);
    expect(text.style._fill!.alpha).toBe(0);
    expect(text.style._stroke!.alpha).toBeCloseTo(128 / 255);
    const bg = container.children[0]!;
    expect(bg.getBounds().y).toBeCloseTo(20);
    expect(bg.getBounds().height).toBeCloseTo(160);
  });
  it('复用 Text 更新内容，移除时以四标志精确销毁', () => {
    const spy = vi.spyOn(Text.prototype, 'destroy');
    try {
      stage.setVisuals([{ type: 'text', x: 1, y: 1, text: 'a' }]);
      const text = stage.root.children[0]!.children[0] as Text;
      stage.setVisuals([{ type: 'text', x: 1, y: 1, text: 'b' }]);
      expect(stage.root.children[0]!.children[0]).toBe(text);
      expect(text.text).toBe('b');
      expect(spy).not.toHaveBeenCalled();
      stage.setVisuals([]);
      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({ style: true, texture: true, textureSource: true }),
      );
    } finally {
      spy.mockRestore();
    }
  });

  it('无效槽位不挪用后续节点；变类与缩短按身份销毁', () => {
    stage.setVisuals(visuals);
    const old = [...stage.root.children];
    const dirty = [{ type: 'banana' }, visuals[1], visuals[0]] as unknown as RoomVisual[];
    stage.setVisuals(dirty);
    expect(stage.root.children[0]).toBe(old[1]);
    expect(stage.root.children[1]).not.toBe(old[2]);
    expect(old[0]!.destroyed).toBe(true);
    expect(old[2]!.destroyed).toBe(true);
    expect(stage.count).toBe(2);
    stage.setVisuals([visuals[2]!, visuals[1]!]);
    expect(stage.root.children[1]).toBe(old[1]);
    expect(stage.count).toBe(2);
  });

  it('复用 Graphics 清除旧路径；新数组中同一原语被修改仍重绘', () => {
    const circle: RoomVisual = { type: 'circle', x: 3, y: 4, style: { radius: 1 } };
    stage.setVisuals([circle]);
    const node = stage.root.children[0] as Graphics, context = node.context;
    circle.x = 30;
    stage.setVisuals([circle]);
    expect(stage.root.children[0]).toBe(node);
    expect(node.context).not.toBe(context);
    expect(node.getBounds().x).toBeCloseTo(2900);
    stage.setVisuals([{ ...circle, style: { fill: 'transparent', stroke: 'transparent' } }]);
    expect(node.context.instructions).toHaveLength(0);
  });

  it('两份几何缓存命中与逐出仍保持画面位置；所有context随节点销毁', () => {
    const a: RoomVisual = { type: 'circle', x: 1, y: 2, style: { radius: 1 } };
    stage.setVisuals([a]);
    const node = stage.root.children[0] as Graphics, c0 = node.context;
    stage.setVisuals([{ ...a, x: 2 }]); const c1 = node.context;
    stage.setVisuals([a]); expect(node.context).toBe(c0);
    expect(node.getBounds().x).toBeCloseTo(0);
    stage.setVisuals([{ ...a, x: 3 }]); expect(node.context).toBe(c1);
    expect(node.getBounds().x).toBeCloseTo(200);
    stage.setVisuals([{ ...a, x: 4 }]); expect(node.context).toBe(c0);
    expect(node.getBounds().x).toBeCloseTo(300);
    stage.setVisuals([]);
    expect(c0.destroyed).toBe(true); expect(c1.destroyed).toBe(true);
  });

  it('连续不同几何只持有两份context，绘制失败也释放当前与备用', () => {
    const contexts = new Set<Graphics['context']>();
    for (let x = 0; x < 40; x++) {
      stage.setVisuals([{ type: 'circle', x, y: 1 }]);
      contexts.add((stage.root.children[0] as Graphics).context);
    }
    expect(contexts.size).toBe(2);
    const paint = vi.spyOn(Graphics.prototype, 'fill').mockImplementationOnce(() => { throw new Error('bad paint'); });
    try {
      stage.setVisuals([{ type: 'circle', x: 41, y: 1 }]);
      expect(stage.count).toBe(0);
      expect([...contexts].every(context => context.destroyed)).toBe(true);
    } finally { paint.mockRestore(); }
    stage.setVisuals([{ type: 'circle', x: 42, y: 1 }]);
    expect(stage.count).toBe(1);
    expect((stage.root.children[0] as Graphics).context.destroyed).toBe(false);
  });

  it('文字样式/背景开关回落默认；同样式不重建 TextStyle', () => {
    const a: RoomVisual = { type: 'text', x: 5, y: 5, text: 'a', style: { stroke: '#ff0000', backgroundColor: '#000000', align: 'left', opacity: 0.2 } };
    stage.setVisuals([a]);
    const container = stage.root.children[0]!, bg = container.children[0] as Graphics;
    const bgContext = bg.context, text = container.children[1] as Text, style = text.style;
    const styleDestroy = vi.spyOn(style, 'destroy');
    stage.setVisuals([{ ...a, text: 'b' }]);
    expect(text.style).toBe(style);
    expect(container.children[0]).toBe(bg);
    stage.setVisuals([{ ...a, style: {} }]);
    expect(container.children).toEqual([text]);
    expect(bgContext.destroyed).toBe(true);
    expect(styleDestroy).toHaveBeenCalledOnce();
    expect(text.style._stroke).toBeNull();
    expect(text.anchor.x).toBe(0.5); expect(text.anchor.y).toBe(1);
    expect(text.alpha).toBe(1);
    stage.setVisuals([a]);
    expect(container.children).toHaveLength(2);
    expect(container.children[1]).toBe(text);
    expect(container.children[0]).not.toBe(bg);
  });

  it('复用文字测量失败释放旧节点和背景；下一批能恢复', () => {
    stage.setVisuals([visuals[4]!]);
    const old = stage.root.children[0]!, text = old.children[1] as Text;
    vi.mocked(CanvasTextMetrics.measureText).mockImplementationOnce(() => { throw new Error('bad measure'); });
    stage.setVisuals([{ ...visuals[4]!, text: 'force remeasure' } as RoomVisual, visuals[0]!]);
    expect(old.destroyed).toBe(true); expect(text.destroyed).toBe(true);
    expect(stage.count).toBe(1);
    stage.setVisuals([visuals[4]!]);
    expect(stage.count).toBe(1);
    expect(stage.root.children[0]!.children).toHaveLength(2);
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
