// after 层"冷静竞争"策略：整房请求视野里的落选者（won=false）证明
// 该目的格发生了竞争；本轮扣住同格胜者，把"抢到的格子"还给落选者
// 重新排队。落选者只在 ctx.args.requests 中可见——胜者输入 movers
// 不含落选者，这正是整房视野的价值。可与任意 replace/before 叠加。
export default function register(api) {
  api.handlers.after('builtin:creep.moveAdmission', { id: 'calm' }, (ctx) => {
    for (const loser of ctx.args.requests.filter((r) => !r.won)) {
      const winner = ctx.args.requests.find((r) => r.won && r.x === loser.x && r.y === loser.y);
      if (winner) ctx.admission.setAdmitted(winner.moverId, false);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
