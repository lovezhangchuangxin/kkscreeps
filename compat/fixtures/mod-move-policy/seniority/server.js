// 完整替换移动竞争排序：ID 最小（最早分配）的竞争者每格优先，其余压低。
// 与默认“MOVE 功率/重量比最高者胜”相反，证明排序策略可独立替换。
export default function register(api) {
  api.handlers.replace('builtin:room.moveContest', { id: 'seniority' }, (ctx) => {
    for (const cell of ctx.args.contests) {
      let oldest = null;
      for (const info of cell.contenders) {
        if (!oldest || info.id < oldest) oldest = info.id;
      }
      for (const info of cell.contenders) {
        ctx.contest.setPriority(info.id, info.id === oldest ? 1 : -1);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
