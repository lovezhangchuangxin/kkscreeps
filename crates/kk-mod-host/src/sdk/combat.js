import { refreshContext } from './refresh.js';

// 能力只借用本次槽的同步访问窗口与故障留存函数。
export function createCombat({ ops, token, fault, write }) {
  function combat(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('战斗能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    function damageRequests(requests) {
      if (!Array.isArray(requests)) throw new Error('批量策略伤害须为数组');
      const count = requests.length;
      if (!Number.isInteger(count) || count < 0 || count > 256)
        throw new Error('批量策略伤害最多 256 项');
      const batch = [];
      for (let i = 0; i < count; i++) {
        const entry = requests[i];
        if (!entry || typeof entry !== 'object') throw new Error('伤害条目须为对象');
        const id = entry.id;
        const amount = entry.amount;
        if (typeof id !== 'string') throw new Error('战斗目标须为对象 ID 的原始字符串');
        if (!Number.isInteger(amount) || amount < 0 || amount > 1e9)
          throw new Error('策略伤害须为 0..=1e9 的整数');
        batch.push({ id, amount });
      }
      return batch;
    }
    return Object.freeze({
      request(targetId) {
        return checked(() => {
          if (typeof targetId !== 'string') throw new Error('战斗目标须为对象 ID 的原始字符串');
          return ops.op_mod_chain_combat_request(token, key, targetId);
        });
      },
      cancel() {
        return checked(() => ops.op_mod_chain_combat_cancel(token, key));
      },
      get(targetId) {
        return checked(() => {
          if (typeof targetId !== 'string') throw new Error('战斗目标须为对象 ID 的原始字符串');
          return ops.op_mod_chain_combat_get(token, key, targetId);
        });
      },
      list(after = '', limit = 100) {
        return checked(() => {
          if (typeof after !== 'string') throw new Error('战斗游标须为 ID 原始字符串或空字符串');
          if (!Number.isInteger(limit) || limit < 1 || limit > 256)
            throw new Error('战斗列表 limit 须为 1..256 的整数');
          return ops.op_mod_chain_combat_list(token, key, after, limit);
        });
      },
      setRetaliation(requests) {
        return checked(() =>
          ops.op_mod_chain_combat_set_retaliation(token, key, damageRequests(requests)),
        );
      },
      applyDamageBatch(requests) {
        return checked(() => {
          const batch = damageRequests(requests);
          return ops.op_mod_chain_combat_apply_damage_batch(token, key, batch);
        });
      },
      applyDamage(targetId, amount) {
        return checked(() => {
          if (typeof targetId !== 'string') throw new Error('战斗目标须为对象 ID 的原始字符串');
          if (!Number.isInteger(amount) || amount < 0 || amount > 1e9) {
            throw new Error('策略伤害须为 0..=1e9 的整数');
          }
          return ops.op_mod_chain_combat_apply_damage(token, key, targetId, amount);
        });
      },
      applyHeal(targetId, amount) {
        return checked(() => {
          if (typeof targetId !== 'string') throw new Error('战斗目标须为对象 ID 的原始字符串');
          if (!Number.isInteger(amount) || amount < 0 || amount > 1e9) {
            throw new Error('策略治疗须为 0..=1e9 的整数');
          }
          return ops.op_mod_chain_combat_apply_heal(token, key, targetId, amount);
        });
      },
    });
  }
  function incoming(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('受击能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      getDamage() {
        return checked(() => ops.op_mod_chain_incoming_damage(token, key));
      },
      setDamage(amount) {
        return checked(() => {
          if (!Number.isSafeInteger(amount) || amount < 0)
            throw new Error('受击伤害须为 JS 安全非负整数');
          return ops.op_mod_chain_incoming_set_damage(token, key, amount);
        });
      },
    });
  }
  return { combat, incoming };
}
