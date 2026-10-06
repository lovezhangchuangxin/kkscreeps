import { refreshContext } from './refresh.js';

// 能力只借用本次槽的同步访问窗口与故障留存函数。
export function createResources({ ops, token, fault, write }) {
  function resources(key, ctx, isOpen) {
    function checked(args, call) {
      try {
        if (!isOpen()) throw new Error('资源能力只能在所属处理器同步执行期间调用');
        for (const value of args) {
          if (typeof value !== 'string') throw new Error('资源 ID 和类型须为原始字符串');
        }
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      read(id, resourceType) {
        return checked([id, resourceType], () => ops.op_mod_chain_resource_read(token, key, id, resourceType));
      },
      transfer(from, to, resourceType, amount, loss = 0) {
        return checked([from, to, resourceType], () => {
          if (typeof amount !== 'number' || typeof loss !== 'number') throw new Error('资源数量须为原始数字');
          // 先验证并暂存当前上下文；资源写入后刷新持久成员和旧库存引用。
          write(key, ctx);
          const result = ops.op_mod_chain_resource_transfer(token, key, from, to, resourceType, amount, loss);
          refreshContext(ctx, ops.op_mod_chain_read(token, key));
          return result;
        });
      },
    });
  }
  return { resources };
}
