// beta 的 manifest 依赖 alpha：只选 beta 时 alpha 被自动加入，
// 且这里的规则覆盖对自动加入的 alpha 生效。
export default function register(api) {
  api.rules.override('alpha:step', 3);
  api.objects.define('beta:beacon', {
    version: 1, playerClass: 'BetaBeacon', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      marks: { type: 'u32', default: 0 },
    },
    tick(ctx) {
      ctx.target.fields.marks += 1;
    },
  });
}
