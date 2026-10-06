// trace 夹具（gatekeeper）：一个可通行/可扣押的对象动作对——pass 正常
// 计数，hold 正常拒绝（api.decline 'gate-closed'），供 --mod-trace 断言
// 拒绝事件携带绑定与 code；对象房间可见，玩家类经 player.js 绑定。
export default function register(api) {
  const door = api.objects.define('gatekeeper:door', {
    version: 1, playerClass: 'GatekeeperDoor', visibility: 'room-visible',
    ownerField: 'owner', positionFields: ['x', 'y'], fields: {
      owner: { type: 'string', maxBytes: 128 },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0 },
    },
  });
  api.intents.define('gatekeeper:pass', {
    scope: 'object', target: door,
    args: { type: 'record', fields: {} },
    process(ctx) {
      ctx.target.fields.value += 1;
      ctx.state = { passed: (ctx.state?.passed || 0) + 1 };
    },
  });
  api.intents.define('gatekeeper:hold', {
    scope: 'object', target: door,
    args: { type: 'record', fields: {} },
    process() {
      return api.decline('gate-closed');
    },
  });
}
