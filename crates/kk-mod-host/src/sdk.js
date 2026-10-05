// @kkscreeps/mod 注册期 SDK。宿主在注册每个包时调用 createApi(namespace, config)，
// 取得 api 对象传入包入口的 default 导出。声明立即经 op 交给 Rust 侧权威校验；
// 处理器函数保存在本 VM 的绑定表里，不跨 isolate 传输。
// seal() 由宿主在全部包注册完成后调用一次，此后任何注册面调用都会抛错。

const declare = (kind, value) => Deno.core.ops.op_mod_declare(kind, value);

let sealed = false;
const bindings = new Map();
const declines = new WeakMap();

// 只有这个工厂生成的返回值表示拒绝，普通对象/错误/返回码保持既有语义。
export function decline(code) {
  if (typeof code !== 'string' || !/^[A-Za-z0-9._:-]{1,128}$/.test(code)) {
    throw new Error('拒绝代码须为 1..128 字节的 ASCII 字母、数字或 ._:-');
  }
  const result = Object.freeze({ code });
  declines.set(result, code);
  return result;
}

function ownKey(namespace, key) {
  if (typeof key !== 'string' || !key.startsWith(namespace + ':')) {
    throw new Error(`键 ${JSON.stringify(key)} 必须以 ${namespace}: 开头`);
  }
  return key;
}

// 跨命名空间键的形状预检（归属由 Rust 侧权威校验；此处只拦明显畸形）。
function anyKey(key, where) {
  if (typeof key !== 'string' || !/^[a-z][a-z0-9_-]{0,63}:[A-Za-z][A-Za-z0-9._-]{0,126}$/.test(key)) {
    throw new Error(`${where} 的键 ${JSON.stringify(key)} 形状无效`);
  }
  return key;
}

function requireFunction(namespace, key, fn, kind) {
  if (sealed) throw new Error('注册已密封');
  ownKey(namespace, key);
  if (typeof fn !== 'function') {
    throw new Error(`绑定 ${key} 需要一个函数`);
  }
  if (bindings.has(key)) throw new Error(`绑定 ${key} 重复注册`);
  bindings.set(key, { fn, kind });
}

function schemaOf(namespace, spec, where) {
  if (spec === undefined || spec === null || typeof spec !== 'object') {
    throw new Error(`${where} 缺少类型说明`);
  }
  const out = { type: spec.type, nullable: spec.nullable === true };
  switch (spec.type) {
    case 'bool':
      return out;
    case 'i32': case 'i64': case 'u32': case 'u64': case 'f64':
      out.min = spec.min;
      out.max = spec.max;
      return out;
    case 'string':
      out.maxBytes = spec.maxBytes;
      return out;
    case 'ref':
      // ref 允许指向其他命名空间的类型（引用与挂载本就跨包）。
      out.target = anyKey(spec.target, where);
      out.version = spec.version;
      return out;
    case 'enum':
      out.values = spec.values;
      return out;
    case 'array':
      out.element = schemaOf(namespace, spec.element, where);
      out.maxLen = spec.maxLen;
      return out;
    case 'map':
      out.value = schemaOf(namespace, spec.value, where);
      out.maxLen = spec.maxLen;
      out.maxKeyBytes = spec.maxKeyBytes;
      return out;
    case 'record':
      out.fields = {};
      for (const [name, member] of Object.entries(spec.fields || {})) {
        out.fields[name] = schemaOf(namespace, member, where);
      }
      out.optional = spec.optional;
      return out;
    default:
      throw new Error(`${where} 的类型 ${JSON.stringify(spec.type)} 未知`);
  }
}

function fieldOf(namespace, name, spec) {
  const where = `字段 ${name}`;
  const field = {
    name,
    schema: schemaOf(namespace, spec, where),
    default: spec.default,
    visibility: spec.visibility === undefined ? 'public' : spec.visibility,
  };
  return field;
}

export function createApi(namespace, config) {
  if (sealed) throw new Error('注册已密封');

  const objects = {
    define(key, spec) {
      ownKey(namespace, key);
      const fields = [];
      for (const [name, value] of Object.entries(spec.fields || {})) {
        fields.push(fieldOf(namespace, name, value));
      }
      let tick;
      if (spec.tick !== undefined) {
        tick = { slot: key + '.tick', process: key + '.tick.process' };
        requireFunction(namespace, tick.process, spec.tick);
      }
      declare('object', {
        key,
        version: spec.version,
        playerClass: spec.playerClass,
        ownerField: spec.ownerField,
        positionFields: spec.positionFields,
        visibility: spec.visibility,
        fields,
        tick,
      });
      return { key, version: spec.version };
    },
  };

  const intents = {
    define(key, spec) {
      ownKey(namespace, key);
      requireFunction(namespace, key + '.process', spec.process);
      declare('intent', {
        key,
        scope: spec.scope,
        target: spec.target,
        args: schemaOf(namespace, spec.args, `动作 ${key}`),
        mode: spec.mode,
        queueMax: spec.queueMax,
        slot: spec.slot,
      });
    },
  };

  const handlers = {
    before: makeHandler('before'),
    wrap: makeHandler('wrap'),
    replace: makeHandler('replace'),
    after: makeHandler('after'),
  };
  function makeHandler(kind) {
    return (slot, spec, fn) => {
      const id = spec && spec.id;
      if (typeof id !== 'string' || !/^[A-Za-z][A-Za-z0-9_.-]{0,126}$/.test(id)) {
        throw new Error(`处理器 id ${JSON.stringify(id)} 须为字母开头的点划线词`);
      }
      const key = ownKey(namespace, `${namespace}:handler.${id}`);
      requireFunction(namespace, key, fn, kind);
      declare('handler', { key, slot, kind, get options() {
        // 原始属性读取留在声明 op 的转换期；原型上的依赖也不能静默丢失。
        const options = Object.create(null);
        for (const name of Object.keys(spec)) options[name] = null;
        options.id = id;
        options.dependsOn = spec.dependsOn;
        return options;
      } });
      return key;
    };
  }

  const rules = {
    define(key, spec) {
      ownKey(namespace, key);
      declare('rule', {
        key,
        type: spec.type,
        min: spec.min,
        max: spec.max,
        default: spec.default,
        consumers: spec.consumers,
      });
    },
    override(rule, value) {
      declare('override', { rule, value });
    },
  };

  const mounts = {
    define(target, name, spec) {
      declare('mount', {
        target,
        field: fieldOf(namespace, name, spec),
      });
    },
  };

  return {
    decline,
    config,
    objects,
    intents,
    handlers,
    rules,
    mounts,
  };
}

export function seal() {
  sealed = true;
  return Array.from(bindings.keys());
}

// 结算期派发：宿主以绑定键调用处理器，ctx 携带 tick/room/state/target/args。
// 处理器同步执行；ctx.state 的终值（含 null=清空）随结果整值写回候选。
// ctx.target.fields 是目标对象的完整字段（对象域动作处理链专用）；
// 处理器原地改写后其终值随结果返回——undefined/null 表示不改动，
// 对象值由宿主按类型描述符校验后整值写入。其余对象读写通道未开放。
// 资源槽的 ctx.resources、移动槽的 ctx.movement、战斗槽的 ctx.combat 与
// 建造族槽的 ctx.build 是各自的受控领域能力：resources 读写同房候选库存，
// movement 读写当前执行者的移动请求登记（pending/request/cancel），combat
// 以新目标执行或取消本战斗动作（request/cancel），build 以新目标执行或
// 取消本建造族动作（request/cancel，进度/伤害/能量数值不经此面）。
// 均不可枚举、不可替换且只在处理器同步期内有效。
// 处理器的持久状态只准走 ctx.state——模块全局不随失败候选回滚，
// 用它做累计会破坏同输入重试的确定性。
export function dispatch(key, ctx) {
  if (!sealed) throw new Error('dispatch 仅在注册密封后可用');
  const entry = bindings.get(key);
  if (!entry || typeof entry.fn !== 'function') {
    throw new Error(`绑定 ${JSON.stringify(key)} 没有注册函数`);
  }
  if (entry.kind === 'wrap') throw new Error('wrap 必须经整槽执行入口调用');
  const result = entry.fn(ctx);
  if (result !== null && (typeof result === 'object' || typeof result === 'function') && declines.has(result)) {
    throw new Error('正常拒绝必须经支持局部回滚的整槽动作入口执行');
  }
  if (result !== undefined && result !== null && typeof result.then === 'function') {
    throw new Error(`绑定 ${JSON.stringify(key)} 的处理器返回了 Promise（结算回调必须同步完成）`);
  }
  const target =
    ctx.target === undefined || ctx.target.fields === undefined
      ? null
      : ctx.target.fields;
  return {
    state: ctx.state === undefined ? null : ctx.state,
    target,
    result: result === undefined ? null : result,
  };
}

// 启动时安装冻结链；运行期只传槽键。JS 控制组合，普通 op 访问 Rust 候选。
const chains = new Map();
export function installChains(programs) {
  if (!sealed || chains.size) throw new Error('冻结链只能在密封后安装一次');
  for (const program of programs) {
    function check(key, kind) {
      const entry = bindings.get(key);
      if (!entry || (kind === 'center' ? entry.kind !== undefined && entry.kind !== 'replace' : entry.kind !== kind)) {
        throw new Error(`绑定 ${key} 与链位 ${kind} 不匹配`);
      }
    }
    for (const key of program.before) check(key, 'before');
    for (const key of program.wraps) check(key, 'wrap');
    if (!program.native_center) check(program.center, 'center');
    for (const key of program.after) check(key, 'after');
    chains.set(program.key, program);
  }
}

// 仅复用自有数据属性，不能把 __proto__ 等继承值当成待刷新容器。
function ownValue(object, key) {
  const descriptor = Object.getOwnPropertyDescriptor(object, key);
  return descriptor && Object.hasOwn(descriptor, 'value') ? descriptor.value : undefined;
}

// Rust 候选是 JSON 树；旧 JS 图中的共享容器只能复用一次，其余路径
// 改用独立候选值，避免第二次刷新覆盖第一次刷新。独立容器保持旧引用。
// keys 用于上下文记录：只更新权威成员，保留处理器附加的本地属性。
function refreshValue(current, fresh, reused, keys) {
  if (current === null || fresh === null || typeof current !== 'object' || typeof fresh !== 'object' || Array.isArray(current) !== Array.isArray(fresh)) {
    return fresh;
  }
  if (reused.has(current)) return fresh;
  reused.add(current);
  if (keys === undefined) {
    for (const key of Object.keys(current)) {
      if (!Object.hasOwn(fresh, key)) delete current[key];
    }
  }
  if (Array.isArray(current)) current.length = fresh.length;
  for (const key of keys ?? Object.keys(fresh)) {
    Object.defineProperty(current, key, { value: refreshValue(ownValue(current, key), fresh[key], reused), writable: true, enumerable: true, configurable: true });
  }
  return current;
}

function refreshContext(ctx, fresh) {
  // state/args/target 共用一次遍历，跨上下文字段的别名也不能重复复用。
  const reused = new WeakSet([ctx]);
  for (const key of ['tick', 'room', 'state', 'args']) {
    Object.defineProperty(ctx, key, { value: refreshValue(ownValue(ctx, key), fresh[key], reused), writable: true, enumerable: true, configurable: true });
  }
  const target = refreshValue(ownValue(ctx, 'target'), fresh.target, reused, ['id', 'fields']);
  Object.defineProperty(ctx, 'target', { value: target, writable: true, enumerable: true, configurable: true });
}

export function runChain(slot, token) {
  const program = chains.get(slot);
  if (!program) throw new Error(`槽 ${slot} 未安装`);
  const ops = Deno.core.ops;
  let firstFault;
  let firstDeclined;
  const declinedSignal = {};
  function fault(error, binding = '', kind = 'chain') {
    if (firstFault !== undefined) return;
    const fallback = binding ? `处理器 ${binding}: 异常诊断格式化失败` : '异常诊断格式化失败';
    // 读取异常属性之前锁定来源并关闭候选；传播时不再次执行诊断 getter。
    firstFault = fallback;
    if (!ops.op_mod_chain_fault(token, binding, kind, fallback)) return;
    let detail = fallback;
    try {
      const message = error && error.message;
      const text = message ? String(message) : String(error);
      if (typeof text === 'string') detail = binding ? `处理器 ${binding}: ${text}` : text;
    } catch {}
    firstFault = detail;
    ops.op_mod_chain_fault_detail(token, detail);
  }
  function checkResult(key, result) {
    if (result !== undefined && result !== null && typeof result.then === 'function') {
      throw new Error(`绑定 ${key} 的处理器返回了 Promise（结算回调必须同步完成）`);
    }
    if (firstFault !== undefined) throw new Error(firstFault);
    // 内层拒绝即使被 wrap 捕获，也阻止外层回写；异常仍由 catch 留存。
    if (firstDeclined !== undefined) throw declinedSignal;
    const code = result !== null && (typeof result === 'object' || typeof result === 'function') ? declines.get(result) : undefined;
    if (code !== undefined) {
      ops.op_mod_chain_decline(token, key, code);
      firstDeclined = code;
      throw declinedSignal;
    }
  }
  function write(key, ctx) {
    // 只序列化持久候选；ctx/target 上的本地函数、循环引用不进入 Rust。
    ops.op_mod_chain_write(token, key, {
      state: ctx.state === undefined ? null : ctx.state,
      target: { fields: ctx.target?.fields ?? null },
    });
  }
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
  function objects(key, ctx, isOpen) {
    function call(strings, action) {
      try {
        if (!isOpen()) throw new Error('对象能力只能在所属处理器同步执行期间调用');
        if (strings.some(value => typeof value !== 'string')) throw new Error('对象 ID、类型和游标须为原始字符串');
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
      get(id) { return call([id], () => ops.op_mod_chain_object_get(token, key, id)); },
      list(typeKey = '', options = {}) {
        return call([typeKey], () => {
          const { after = '', limit = 128 } = options;
          if (typeof after !== 'string' || !Number.isInteger(limit) || limit < 1 || limit > 256) {
            throw new Error('对象列表游标须为字符串，limit 须为 1..256 的整数');
          }
          return ops.op_mod_chain_object_list(token, key, typeKey, after, limit);
        });
      },
      create(typeKey, fields) { return call([typeKey], () => ops.op_mod_chain_object_create(token, key, typeKey, fields)); },
      update(id, fields) { return call([id], () => ops.op_mod_chain_object_update(token, key, id, fields)); },
      remove(id) { return call([id], () => ops.op_mod_chain_object_remove(token, key, id)); },
    });
  }
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
      pending() { return checked(() => ops.op_mod_chain_move_pending(token, key)); },
      request(direction) {
        return checked(() => {
          if (typeof direction !== 'number' || !Number.isInteger(direction) || direction < 1 || direction > 8) {
            throw new Error('移动方向须为 1..=8 的整数');
          }
          return ops.op_mod_chain_move_request(token, key, direction);
        });
      },
      cancel() { return checked(() => ops.op_mod_chain_move_cancel(token, key)); },
    });
  }
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
    return Object.freeze({
      request(targetId) {
        return checked(() => {
          if (typeof targetId !== 'string') throw new Error('战斗目标须为对象 ID 的原始字符串');
          return ops.op_mod_chain_combat_request(token, key, targetId);
        });
      },
      cancel() { return checked(() => ops.op_mod_chain_combat_cancel(token, key)); },
    });
  }
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
      cancel() { return checked(() => ops.op_mod_chain_build_cancel(token, key)); },
    });
  }
  function simple(key) {
    const ctx = ops.op_mod_chain_read(token, key);
    let open = true;
    Object.defineProperty(ctx, 'resources', { value: resources(key, ctx, () => open) });
    Object.defineProperty(ctx, 'objects', { value: objects(key, ctx, () => open) });
    Object.defineProperty(ctx, 'movement', { value: movement(key, () => open) });
    Object.defineProperty(ctx, 'combat', { value: combat(key, () => open) });
    Object.defineProperty(ctx, 'build', { value: build(key, ctx, () => open) });
    try {
      checkResult(key, bindings.get(key).fn(ctx));
      write(key, ctx);
    } catch (error) {
      if (error === declinedSignal) throw error;
      fault(error, key, 'handler');
      throw error;
    } finally {
      open = false;
    }
  }
  function inner(depth) {
    if (depth === program.wraps.length) {
      if (program.native_center) ops.op_mod_chain_native(token);
      else simple(program.center);
      return;
    }
    const key = program.wraps[depth];
    const ctx = ops.op_mod_chain_read(token, key);
    let called = false;
    let open = true;
    Object.defineProperty(ctx, 'resources', { value: resources(key, ctx, () => open) });
    Object.defineProperty(ctx, 'objects', { value: objects(key, ctx, () => open) });
    Object.defineProperty(ctx, 'movement', { value: movement(key, () => open) });
    Object.defineProperty(ctx, 'combat', { value: combat(key, () => open) });
    Object.defineProperty(ctx, 'build', { value: build(key, ctx, () => open) });
    function next() {
      if (!open) throw new Error('next 只能在所属处理器同步执行期间调用');
      if (called) {
        const error = new Error('wrap 处理器重复调用 next，本轮候选失败');
        fault(error, key, 'control');
        throw error;
      }
      called = true;
      try {
        // 外层修改先写候选，内层按绑定读取新状态；返回后刷新外层视图。
        write(key, ctx);
        inner(depth + 1);
        refreshContext(ctx, ops.op_mod_chain_read(token, key));
      } catch (error) {
        if (error === declinedSignal) throw error;
        fault(error, key, 'context');
        throw error;
      }
    }
    try {
      checkResult(key, bindings.get(key).fn(ctx, next));
      if (firstFault !== undefined) throw new Error(firstFault);
      if (firstDeclined !== undefined) throw declinedSignal;
      write(key, ctx);
    } catch (error) {
      if (error === declinedSignal) throw error;
      fault(error, key, 'handler');
      throw error;
    } finally {
      open = false;
    }
  }
  try {
    for (const key of program.before) simple(key);
    inner(0);
    if (firstFault !== undefined) throw new Error(firstFault);
    // after 属于整个槽：wrap 全部退栈后执行一次；取消中心仍可成功返回。
    for (const key of program.after) simple(key);
  } catch (error) {
    if (error === declinedSignal) return;
    fault(error);
    throw firstFault;
  }
}
