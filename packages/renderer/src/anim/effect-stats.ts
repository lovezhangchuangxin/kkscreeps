/**
 * 动效诊断统计形状（普通数据；零 pixi 导入——公共 d.ts 闭包守卫要求，评审
 * B-B3：EffectsStats/AmbientStats 原住 pixi 模块，经 RendererStats 转手会把
 * pixi 类型拉进公共类型闭包）。语义归 effects-stage/ambient-scheduler 所有。
 */

/** 有限效果运行时统计（EffectsStage.stats）。 */
export interface EffectsStats {
  active: number;
  nodes: number;
  texts: number;
  skipped: number;
}

/** 常驻动画调度统计（AmbientScheduler.stats）。 */
export interface AmbientStats {
  active: number;
  awake: number;
  sleeping: number;
}
