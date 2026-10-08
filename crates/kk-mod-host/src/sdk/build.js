import { refreshContext } from './refresh.js';

// 能力只借用本次槽的同步访问窗口与故障留存函数。
export function createBuild({ ops, token, fault, write }) {
  function build(key, ctx, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('建造能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      request(targetId) {
        return checked(() => {
          if (typeof targetId !== 'string') throw new Error('建造目标须为对象 ID 的原始字符串');
          // 建造族 native 改执行者能量/耐久等候选字段；先验证并暂存当前
          // 上下文，执行后刷新持久成员，避免处理器返回时的字段写回把
          // 旧视图当修改（与资源转账同一坑）。
          write(key, ctx);
          const result = ops.op_mod_chain_build_request(token, key, targetId);
          refreshContext(ctx, ops.op_mod_chain_read(token, key));
          return result;
        });
      },
      cancel() {
        return checked(() => ops.op_mod_chain_build_cancel(token, key));
      },
    });
  }
  return { build };
}
