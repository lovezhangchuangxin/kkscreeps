import { refreshContext } from './refresh.js';

// 能力只借用本次槽的同步访问窗口与故障留存函数。
export function createObjects({ ops, token, fault, write }) {
  function objects(key, ctx, isOpen) {
    function call(strings, action) {
      try {
        if (!isOpen()) throw new Error('对象能力只能在所属处理器同步执行期间调用');
        if (strings.some((value) => typeof value !== 'string'))
          throw new Error('对象 ID、类型和游标须为原始字符串');
        // 读面也先同步本处理器修改，所有查询都观察当前候选。
        write(key, ctx);
        const result = action();
        refreshContext(ctx, ops.op_mod_chain_read(token, key));
        return result;
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      get(id) {
        return call([id], () => ops.op_mod_chain_object_get(token, key, id));
      },
      list(typeKey = '', options = {}) {
        return call([typeKey], () => {
          const { after = '', limit = 128 } = options;
          if (typeof after !== 'string' || !Number.isInteger(limit) || limit < 1 || limit > 256) {
            throw new Error('对象列表游标须为字符串，limit 须为 1..256 的整数');
          }
          return ops.op_mod_chain_object_list(token, key, typeKey, after, limit);
        });
      },
      create(typeKey, fields) {
        return call([typeKey], () => ops.op_mod_chain_object_create(token, key, typeKey, fields));
      },
      update(id, fields) {
        return call([id], () => ops.op_mod_chain_object_update(token, key, id, fields));
      },
      remove(id) {
        return call([id], () => ops.op_mod_chain_object_remove(token, key, id));
      },
    });
  }
  return { objects };
}
