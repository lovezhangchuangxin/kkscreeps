// after 层功率让行：配重过剩（movePower>weight，即多 MOVE 快车）
// 的请求让行。请求级语义：快车的**请求**被否决后不参与竞争，
// 同格慢车独占目的格胜出——与障碍槽挡下胜者后无人可动形成对照。
// 可与任意 replace/before 叠加，否决集在同一候选上取并集。
export default function register(api) {
  api.handlers.after('builtin:room.movePrecheck', { id: 'courtesy' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.movePower > r.weight && !ctx.precheck.isBlocked(r.moverId)) {
        ctx.precheck.block(r.moverId);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
