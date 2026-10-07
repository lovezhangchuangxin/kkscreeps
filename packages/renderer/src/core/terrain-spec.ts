/**
 * 地形渲染规格常量——〔官拆〕§6（screeps/renderer terrain.js/metadata/index.js
 * 逐行取证，F2 方案 §2.2/§6 验收口径）。测试直接断言本表；实现只许引用本表
 * 不许内联魔法数。
 */

export const TERRAIN_SPEC = {
  /** plain：底色 + ground 噪声平铺 + ground-mask 乘算（lighting normal 档）。 */
  plain: {
    color: 0x555555,
    groundAlpha: 0.3,
    groundTileScale: 3,
    groundMaskTileScale: 7,
    groundMaskAlpha: 0.15,
    groundMaskBlend: 'multiply',
  },
  /** swamp：合并 path 填充 + 双白罩（兼噪声 mask）+ 双 noise2 ADD 滚动。 */
  swamp: {
    fill: '#4a501e',
    strokeWidth: 50,
    alpha: 0.4,
    overlayFill: '#ffffff',
    overlayAlpha: 0.25,
    noiseTint: 0x66ff00,
    noiseAlpha: 0.3,
    noiseTileScales: [10, 14],
    /** Ticker 帧 delta（60fps 基准）×drift，(dx,dy)×两层；官方 animated 档。 */
    noiseDrift: [1.5, 1.5, -1.35, -1.35],
    noiseBlend: 'add',
  },
  /** wall：合并 path 烘焙 + noise1 bump（ADD）+ lighting 层黑影（blur）+ 亮填。 */
  wall: {
    fill: '#111111',
    stroke: '#000000',
    strokeWidth: 10,
    bumpAlpha: 0.2,
    /** 官方 tileScale=8×sizeRatio（sizeRatio=renderSize/view）。 */
    bumpTileScaleFactor: 8,
    shadowFill: '#000000',
    /** 官方 WALLS_BLUR：blur 强度 = renderSize × 0.006（px）。 */
    shadowBlurRatio: 0.006,
    lightFill: '#808080',
    lightStroke: '#000000',
    /** lighting 层内混合（官方 display 钩子语义：成员默认 SCREEN，_overrideBlend 为 MULTIPLY）。 */
    shadowInLayerBlend: 'multiply',
    fillInLayerBlend: 'screen',
  },
  /** 出口：四边 TilingSprite，ADD α0.5，tileScale=cell/纹宽。 */
  exit: {
    alpha: 0.5,
    blend: 'add',
  },
  /** lighting 层：0x808080 环境光整片，层输出以 MULTIPLY 合成回场景（G2）。 */
  lighting: {
    ambient: 0x808080,
    ambientAlpha: 1.0,
  },
  /** 烘焙画布边长（官方 RENDER_SIZE，demo worldConfigs）。 */
  renderSize: 2048,
  /** 渲染循环帧基准（60fps；swamp 滚动时基/ ticker delta 换算共用——C-S8 单源）。 */
  frameBaseMs: 1000 / 60,
} as const;
