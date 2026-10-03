// @kkscreeps/mod 注册期 SDK。宿主在注册每个包时调用 createApi(namespace, config)，
// 取得 api 对象传入包入口的 default 导出。声明立即经 op 交给 Rust 侧权威校验；
// 处理器函数保存在本 VM 的绑定表里，不跨 isolate 传输。
// seal() 由宿主在全部包注册完成后调用一次，此后任何注册面调用都会抛错。

const declare = (kind, value) => Deno.core.ops.op_mod_declare(kind, value);

let sealed = false;
const bindings = new Map();

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
      declare('object', {
        key,
        version: spec.version,
        playerClass: spec.playerClass,
        ownerField: spec.ownerField,
        positionFields: spec.positionFields,
        visibility: spec.visibility,
        fields,
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
      declare('handler', { key, slot, kind });
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
  function fault(error) {
    const detail = error && error.message ? String(error.message) : String(error);
    if (firstFault === undefined) firstFault = detail;
    ops.op_mod_chain_fault(token, detail);
  }
  function checkResult(key, result) {
    if (result !== undefined && result !== null && typeof result.then === 'function') {
      throw new Error(`绑定 ${key} 的处理器返回了 Promise（结算回调必须同步完成）`);
    }
  }
  function write(key, ctx) {
    // 只序列化持久候选；ctx/target 上的本地函数、循环引用不进入 Rust。
    ops.op_mod_chain_write(token, key, {
      state: ctx.state === undefined ? null : ctx.state,
      target: { fields: ctx.target?.fields ?? null },
    });
  }
  function simple(key) {
    const ctx = ops.op_mod_chain_read(token, key);
    try {
      checkResult(key, bindings.get(key).fn(ctx));
      write(key, ctx);
    } catch (error) {
      fault(new Error(`处理器 ${key}: ${error && error.message ? error.message : error}`));
      throw error;
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
    function next() {
      if (!open) throw new Error('next 只能在所属处理器同步执行期间调用');
      if (called) {
        const error = new Error('wrap 处理器重复调用 next，本轮候选失败');
        fault(error);
        throw error;
      }
      called = true;
      try {
        // 外层修改先写候选，内层按绑定读取新状态；返回后刷新外层视图。
        write(key, ctx);
        inner(depth + 1);
        refreshContext(ctx, ops.op_mod_chain_read(token, key));
      } catch (error) {
        fault(error);
        throw error;
      }
    }
    try {
      checkResult(key, bindings.get(key).fn(ctx, next));
      if (firstFault !== undefined) throw new Error(firstFault);
      write(key, ctx);
    } catch (error) {
      fault(new Error(`处理器 ${key}: ${error && error.message ? error.message : error}`));
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
    fault(error);
    throw error;
  }
}
