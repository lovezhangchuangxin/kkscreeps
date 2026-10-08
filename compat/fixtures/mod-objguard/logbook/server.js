// 独立记录型包：objguard-logbook:monolith 声明耐久 200；包在
// builtin:mod.receiveDamage 挂 After 观察者——不改伤害量，把当次输入
//（伤害/来源/扣减前耐久读值）记入 namespace state。与减伤型包同计划
// 共存时观察链尾的最终伤害量（Replace 先执行）。
export default function register(api) {
  api.objects.define('objguard-logbook:monolith', {
    version: 1,
    playerClass: 'LogbookMonolith',
    positionFields: ['x', 'y'],
    hitsMax: 200,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
  });
  api.handlers.after('builtin:mod.receiveDamage', { id: 'record' }, ctx => {
    ctx.state = {
      calls: (ctx.state?.calls || 0) + 1,
      finalDamage: ctx.incoming.getDamage(),
      sawSource: ctx.args.source !== null,
      hitsBefore: ctx.target.fields.hits,
    };
  });
}
