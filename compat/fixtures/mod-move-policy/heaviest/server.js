// after 层调整：把每格移动重量最大的竞争者抬到 +2，覆盖 before/中心的
// 相对排序；其余竞争者保持已有优先级（组合时可观察到叠加语义）。
export default function register(api) {
  api.handlers.after('builtin:room.moveContest', { id: 'heaviest' }, (ctx) => {
    for (const cell of ctx.args.contests) {
      let heaviest = null;
      let best = -1;
      for (const info of cell.contenders) {
        if (info.weight > best) {
          best = info.weight;
          heaviest = info.id;
        }
      }
      ctx.contest.setPriority(heaviest, 2);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
