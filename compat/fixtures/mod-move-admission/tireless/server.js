// replace 层放行：所有获胜执行者一律准入，忽略疲劳/MOVE 部件门槛。
// 初值即原生 can_move 结果；覆盖只扩大获准集合，不改变请求矩阵。
export default function register(api) {
  api.handlers.replace('builtin:creep.moveAdmission', { id: 'tireless' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      ctx.admission.setAdmitted(mover.id, true);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
