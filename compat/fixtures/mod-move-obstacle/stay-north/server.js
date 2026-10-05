// replace 层追加阻挡：目的格 y>=20（南下）的胜者全部拦截——北半区
// 交通管制。单向 block 只能收紧，不能放行 native 判定的阻挡。
export default function register(api) {
  api.handlers.replace('builtin:room.moveObstacle', { id: 'stayNorth' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.targetY >= 20) {
        ctx.obstacle.block(mover.id);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
