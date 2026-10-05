// 内容选择夹具：alpha 声明规则驱动的自有对象与玩家面，不依赖其他包。
export default function register(api) {
  api.rules.define('alpha:step', {
    type: 'i32', min: 1, max: 9, default: 1, consumers: ['alpha:node.tick.process'],
  });
  api.objects.define('alpha:node', {
    version: 1, playerClass: 'AlphaNode', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'u32', default: 0 },
    },
    tick(ctx) {
      ctx.target.fields.value += api.rules.effective('alpha:step');
    },
  });
}
