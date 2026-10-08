import { describe, expect, it, vi } from 'vitest';
import {
  AlphaKeyframesTween,
  AlphaTween,
  MoveTween,
  RotateTween,
  TweenManager,
  easeInOutQuad,
  shortestAngleDelta,
} from '../src/anim/tweens.js';

// 官方动效语义的解析式等价（tweens.ts 头注锚点）。

describe('easeInOutQuad（easingHelper.js easeInOut(2)）', () => {
  it('端点与对称性', () => {
    expect(easeInOutQuad(0)).toBe(0);
    expect(easeInOutQuad(0.5)).toBe(0.5);
    expect(easeInOutQuad(1)).toBe(1);
  });
  it('分段公式 t<0.5 ? 2t² : 1−2(1−t)²', () => {
    expect(easeInOutQuad(0.25)).toBeCloseTo(2 * 0.25 * 0.25);
    expect(easeInOutQuad(0.75)).toBeCloseTo(1 - 2 * 0.25 * 0.25);
  });
});

describe('shortestAngleDelta（RotateTo.js:17-24 归一）', () => {
  it('短路径直接差', () => {
    expect(shortestAngleDelta(0, Math.PI / 2)).toBeCloseTo(Math.PI / 2);
    expect(shortestAngleDelta(0.5, -0.5)).toBeCloseTo(-1);
  });
  it('跨 ±π 取最短（3 → −3 走 +0.283 经 π）', () => {
    const d = shortestAngleDelta(3, -3);
    expect(d).toBeGreaterThan(0);
    expect(d).toBeCloseTo(2 * Math.PI - 6);
  });
});

function point(x: number, y: number): { x: number; y: number } {
  return { x, y };
}

describe('MoveTween（Ease(MoveTo) 解析等价）', () => {
  it('easeInOutQuad 位置插值 + 终点吸附', () => {
    const p = point(0, 0);
    const tween = new MoveTween(
      (x, y) => {
        p.x = x;
        p.y = y;
      },
      0,
      0,
      100,
      200,
      1000,
    );
    tween.update(250); // t=0.25 → eased 2·0.25²=0.125
    expect(p.x).toBeCloseTo(12.5);
    expect(p.y).toBeCloseTo(25);
    tween.update(250); // t=0.5 → 0.5
    expect(p.x).toBeCloseTo(50);
    tween.update(500); // 完成 → 吸附
    expect(p.x).toBe(100);
    expect(p.y).toBe(200);
  });
  it('中途反向重定位（cancel 保留当前位，新段自当前起）', () => {
    const p = point(0, 0);
    const a = new MoveTween(
      (x, y) => {
        p.x = x;
        p.y = y;
      },
      0,
      0,
      100,
      0,
      1000,
    );
    a.update(500); // 半程 50
    expect(p.x).toBeCloseTo(50);
    const b = new MoveTween(
      (x, y) => {
        p.x = x;
        p.y = y;
      },
      p.x,
      p.y,
      150,
      0,
      1000,
    );
    b.update(1000);
    expect(p.x).toBe(150);
  });
  it('时长 0 → 首帧吸附终点', () => {
    const p = point(0, 0);
    const t = new MoveTween(
      (x, y) => {
        p.x = x;
        p.y = y;
      },
      0,
      0,
      42,
      0,
      0,
    );
    expect(t.update(16)).toBe(true);
    expect(p.x).toBe(42);
  });
});

describe('RotateTween（无 Ease 线性 + 最短角）', () => {
  it('线性插值与终值', () => {
    let r = 0;
    const t = new RotateTween(
      (v) => {
        r = v;
      },
      0,
      Math.PI / 2,
      200,
    );
    t.update(100);
    expect(r).toBeCloseTo(Math.PI / 4);
    t.update(100);
    expect(r).toBeCloseTo(Math.PI / 2);
  });
  it('3 → −3 走最短正向经 π', () => {
    let r = 3;
    const t = new RotateTween(
      (v) => {
        r = v;
      },
      3,
      -3,
      1000,
    );
    t.update(1000);
    expect(r).toBeCloseTo(-3 + 2 * Math.PI); // −3 经正向 0.283 → 3+0.283=3.283≡−3+2π
  });
});

describe('AlphaTween（FadeOut 线性）', () => {
  it('1→0 半程 0.5、结束 0', () => {
    let a = -1;
    const t = new AlphaTween(
      (v) => {
        a = v;
      },
      1,
      0,
      500,
    );
    t.update(250);
    expect(a).toBeCloseTo(0.5);
    expect(t.update(250)).toBe(true);
    expect(a).toBe(0);
  });
});

describe('AlphaKeyframesTween（受击罩 0→0.5@1/4→0@1）', () => {
  it('分段线性', () => {
    let a = -1;
    const t = new AlphaKeyframesTween(
      (v) => {
        a = v;
      },
      [
        [0.25, 0.5],
        [1, 0],
      ],
      1000,
    );
    t.update(250);
    expect(a).toBeCloseTo(0.5);
    t.update(375); // f=0.625 → 下降段（0.25→1）的中点
    expect(a).toBeCloseTo(0.25);
    expect(t.update(375)).toBe(true);
    expect(a).toBe(0);
  });
});

describe('TweenManager（ActionManager 语义）', () => {
  it('update 推进并在结束时移除 + onEnd', () => {
    const manager = new TweenManager();
    let a = 1;
    let ended = 0;
    manager.run(
      new AlphaTween(
        (v) => {
          a = v;
        },
        1,
        0,
        100,
      ),
      () => {
        ended += 1;
      },
    );
    manager.update(50);
    expect(a).toBeCloseTo(0.5);
    expect(ended).toBe(0);
    manager.update(50);
    expect(a).toBe(0);
    expect(ended).toBe(1);
    expect(manager.size).toBe(0);
  });
  it('cancel 摘除不触发 onEnd、不 finish（官方 cancelAction 语义）', () => {
    const manager = new TweenManager();
    let a = 1;
    let ended = 0;
    const t = new AlphaTween(
      (v) => {
        a = v;
      },
      1,
      0,
      100,
    );
    manager.run(t, () => {
      ended += 1;
    });
    manager.update(50);
    manager.cancel(t);
    manager.update(50);
    expect(a).toBeCloseTo(0.5); // 停在被摘除时的值
    expect(ended).toBe(0);
  });
  it('迭代内 onEnd 新增/取消安全（死亡淡出 onEnd 销毁视图的路径）', () => {
    const manager = new TweenManager();
    const onEnd = vi.fn(() => {
      manager.run(new AlphaTween(() => {}, 1, 0, 10)); // onEnd 中再 run
    });
    manager.run(new AlphaTween(() => {}, 1, 0, 10), onEnd);
    manager.update(10);
    expect(onEnd).toHaveBeenCalledTimes(1);
    manager.update(10); // 新 tween 完成
    expect(manager.size).toBe(0);
  });
  it('迭代内 run 后同帧 cancel → 不经 pending 复活（评审 C建议5 回归锚）', () => {
    const manager = new TweenManager();
    let a = 1;
    const inner = new AlphaTween(
      (v) => {
        a = v;
      },
      1,
      0,
      10,
    );
    manager.run(new AlphaTween(() => {}, 1, 0, 10), () => {
      manager.run(inner); // 迭代内 run → pending
      manager.cancel(inner); // 同帧 cancel → 不得复活
    });
    manager.update(10);
    expect(manager.size).toBe(0);
    const before = a;
    manager.update(10);
    expect(a).toBe(before); // inner 未被推进
  });
});
