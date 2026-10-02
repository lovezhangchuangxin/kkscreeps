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

function requireFunction(namespace, key, fn) {
  if (sealed) throw new Error('注册已密封');
  ownKey(namespace, key);
  if (typeof fn !== 'function') {
    throw new Error(`绑定 ${key} 需要一个函数`);
  }
  if (bindings.has(key)) throw new Error(`绑定 ${key} 重复注册`);
  bindings.set(key, fn);
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
      requireFunction(namespace, key, fn);
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
