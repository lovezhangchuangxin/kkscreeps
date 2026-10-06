// trace 夹具（breaker）：link tick 前置处理器固定抛错，供 --mod-trace
// 断言 slot_fault（槽/绑定/错误文本）与 settlement_retry 事件；sim 默认
// 不配自动停用阈值，故障轮持续缓存重试。
export default function register(api) {
  api.handlers.before('builtin:link.tick', { id: 'boom' }, () => {
    throw Error('breaker-boom');
  });
}
