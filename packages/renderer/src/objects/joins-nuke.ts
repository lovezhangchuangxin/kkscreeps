import { Container, Graphics, Sprite } from 'pixi.js';
import type { JoinAssets, PartJoin } from './joins-naturals.js';
import { makeGlow } from './draw-helpers.js';

export function joinNuke(assets: JoinAssets): PartJoin {
  return {
    marker: {
      create: () => {
        const root = new Container();
        for (const [name, alpha] of [
          ['nuke', 0.7],
          ['fx-nuke-blur', 1],
        ] as const) {
          const texture = assets.frames.get(name);
          if (!texture) continue;
          const s = new Sprite(texture);
          s.anchor.set(0.5);
          s.alpha = alpha;
          if (name === 'fx-nuke-blur') s.blendMode = 'screen';
          root.addChild(s);
        }
        return root;
      },
      apply: () => {},
    },
    circle: {
      create: () => {
        const g = new Graphics().circle(0, 0, 110).fill(0xff2222);
        g.alpha = 0.5;
        g.blendMode = 'add';
        return g;
      },
      apply: () => {},
    },
    pulse: {
      create: () => {
        const root = new Container();
        root.alpha = 0.4;
        for (let i = 0; i < 4; i++) {
          const texture = assets.frames.get(`fx-nuke-pulse-${i}`);
          if (!texture) continue;
          const s = new Sprite(texture);
          s.label = `blur-${i}`;
          s.anchor.set(0.5);
          s.setSize(1440);
          s.tint = 0xff0000;
          s.alpha = i === 0 ? 1 : 0;
          s.blendMode = 'add';
          root.addChild(s);
        }
        return root;
      },
      apply: () => {},
    },
    glow: { create: () => makeGlow(assets.glow, 700, 700, 1, 0xff4444), apply: () => {} },
  };
}
