// after 层追加阻挡：目的格上有获准让位中的 creep（yielding）时拦截，
// 禁止依赖让位链——交换/环移全部失效，独立移动不受影响。可与任意
// replace/before 叠加，追加集在同一候选上取并集。
export default function register(api) {
  api.handlers.after('builtin:room.moveObstacle', { id: 'noSwap' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.objects.some((o) => o.kind === 'creep' && o.yielding)) {
        ctx.obstacle.block(mover.id);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
