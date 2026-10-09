import { refreshContext } from './refresh.js';

// 能力只借用本次槽的同步访问窗口与故障留存函数。
export function createMovement({ ops, token, fault, write }) {
  function movement(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('移动请求能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      pending() {
        return checked(() => ops.op_mod_chain_move_pending(token, key));
      },
      request(direction) {
        return checked(() => {
          if (
            typeof direction !== 'number' ||
            !Number.isInteger(direction) ||
            direction < 1 ||
            direction > 8
          ) {
            throw new Error('移动方向须为 1..=8 的整数');
          }
          return ops.op_mod_chain_move_request(token, key, direction);
        });
      },
      cancel() {
        return checked(() => ops.op_mod_chain_move_cancel(token, key));
      },
    });
  }
  function contest(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('移动竞争能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      getPriority(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('竞争者 ID 须为原始字符串');
          return ops.op_mod_chain_contest_priority(token, key, id);
        });
      },
      setPriority(id, priority) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('竞争者 ID 须为原始字符串');
          if (!Number.isSafeInteger(priority)) throw new Error('竞争优先级须为 JS 安全整数');
          return ops.op_mod_chain_contest_set_priority(token, key, id, priority);
        });
      },
    });
  }
  // 房间级准入槽的 ctx.admission 读写本次输入中执行者的准入终值；
  // 初值即原生 can_move 结果，覆盖只影响获准集合，不开放请求重排。
  // evaluate 按候选当前终值预演取消链（只读，不写世界）。
  function admission(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('移动准入能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      getAdmitted(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('执行者 ID 须为原始字符串');
          return ops.op_mod_chain_admission_admitted(token, key, id);
        });
      },
      setAdmitted(id, admitted) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('执行者 ID 须为原始字符串');
          if (typeof admitted !== 'boolean') throw new Error('准入终值须为布尔值');
          return ops.op_mod_chain_admission_set_admitted(token, key, id, admitted);
        });
      },
      evaluate() {
        return checked(() => ops.op_mod_chain_admission_evaluate(token, key));
      },
    });
  }
  // 房间级预检槽累积静态放行与请求否决；放行不能清除显式否决或动态占格。
  function precheck(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('移动预检能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      isBlocked(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('请求者 ID 须为原始字符串');
          return ops.op_mod_chain_precheck_blocked(token, key, id);
        });
      },
      block(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('请求者 ID 须为原始字符串');
          return ops.op_mod_chain_precheck_block(token, key, id);
        });
      },
      allowStatic(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('请求者 ID 须为原始字符串');
          return ops.op_mod_chain_precheck_allow_static(token, key, id);
        });
      },
    });
  }
  // 房间级拉拽登记槽的 ctx.pull 追加配对否决；单向能力，不提供放行或新增。
  function pull(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('拉拽登记能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      isDenied(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('拉者 ID 须为原始字符串');
          return ops.op_mod_chain_pull_denied(token, key, id);
        });
      },
      deny(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('拉者 ID 须为原始字符串');
          return ops.op_mod_chain_pull_deny(token, key, id);
        });
      },
    });
  }
  // 房间级代价槽的 ctx.cost 读写疲劳覆盖；数值替换，缺项为原生代价。
  function cost(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('移动代价能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      getFatigue(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('移动者 ID 须为原始字符串');
          return ops.op_mod_chain_cost_fatigue_of(token, key, id);
        });
      },
      setFatigue(id, value) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('移动者 ID 须为原始字符串');
          if (!Number.isSafeInteger(value) || value < 0)
            throw new Error('移动代价须为非负安全整数');
          return ops.op_mod_chain_cost_set_fatigue(token, key, id, value);
        });
      },
    });
  }
  // 房间级障碍槽的 ctx.obstacle 追加阻挡；单向能力，不提供放行。
  function obstacle(key, isOpen) {
    function checked(call) {
      try {
        if (!isOpen()) throw new Error('移动障碍能力只能在所属处理器同步执行期间调用');
        return call();
      } catch (error) {
        fault(error, key, 'context');
        throw error;
      }
    }
    return Object.freeze({
      isBlocked(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('胜者 ID 须为原始字符串');
          return ops.op_mod_chain_obstacle_blocked(token, key, id);
        });
      },
      block(id) {
        return checked(() => {
          if (typeof id !== 'string') throw new Error('胜者 ID 须为原始字符串');
          return ops.op_mod_chain_obstacle_block(token, key, id);
        });
      },
      evaluate() {
        return checked(() => ops.op_mod_chain_obstacle_evaluate(token, key));
      },
    });
  }
  return { movement, contest, admission, precheck, pull, cost, obstacle };
}
