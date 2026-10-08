// 注册与声明预检，权威校验仍由 Rust 执行。
import { sealed, bindings, declines, effectiveRules, rulesInstalled, sealState } from './state.js';
const declare = (kind, value) => Deno.core.ops.op_mod_declare(kind, value);

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
export function anyKey(key, where) {
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
        capacity: spec.capacity,
        hitsMax: spec.hitsMax,
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
        // 压制声明（G07.b）：数组内为压制者动作的完整键；冻结校验要求
        // 同 scope 同接收者，引擎按同 tick 键存在性判定（见架构文档）。
        suppressedBy: spec.suppressedBy,
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
    // 读取冻结计划的有效规则值（含默认、单方覆盖与显式选择的多方覆盖）。
    // 只在宿主安装冻结规则后可用；键为完整稳定键，可跨命名空间读取。
    // 返回原始 number：整数按双精度转换，超过 ±(2^53-1) 时取最近双精度值
    // （与 ctx.state/args 写回候选的 serde_json 转换同一语义，不用 BigInt）；
    // 这是只读快照，不提供运行期改写。
    effective(key) {
      if (!rulesInstalled) {
        throw new Error('有效规则在注册与冻结完成前不可读取（不能读取声明时的默认值）');
      }
      anyKey(key, '规则读取');
      if (!effectiveRules.has(key)) {
        throw new Error(`未知规则键 ${JSON.stringify(key)}（不在当前冻结计划内）`);
      }
      return effectiveRules.get(key);
    },
  };

  // 命名空间资源目录：键即玩家面与库存槽中的资源名（如 demo:crystal）。
  // 声明经 op 交给 Rust 权威校验（归属、重复与计划合并在冻结期定论）。
  // 首片没有逐资源数值——容量沿既有仓库规则面；市场/跨房不在目录消费面内。
  const resources = {
    define(key, spec) {
      ownKey(namespace, key);
      if (spec !== undefined) {
        if (spec === null || typeof spec !== 'object' || Array.isArray(spec)) {
          throw new Error(`资源 ${key} 的说明须为对象`);
        }
        const unknown = Object.keys(spec)[0];
        if (unknown !== undefined) {
          // 当前没有已知选项；拼错形状静默落空等于改写目录语义。
          throw new Error(`资源 ${key} 声明了未知选项 ${unknown}`);
        }
      }
      declare('resource', { key });
      return key;
    },
  };

  // 可建造建筑声明：纯数据配方（成本向量/耐久/寻路语义/容量/生产配方），
  // 由宿主通用执行器消费——工地、建造进度、受击维修、库存与生产不需要
  // 作者提供处理器。键即完工结构的 structureType（玩家面类型名）。
  const buildings = {
    define(key, spec) {
      ownKey(namespace, key);
      if (spec === null || typeof spec !== 'object' || Array.isArray(spec)) {
        throw new Error(`建筑 ${key} 的说明须为对象`);
      }
      const known = ['playerClass', 'cost', 'hitsMax', 'pathing', 'capacity', 'production', 'fields', 'tick'];
      for (const member of Object.keys(spec)) {
        if (known.indexOf(member) === -1) {
          throw new Error(`建筑 ${key} 声明了未知选项 ${member}`);
        }
      }
      if (typeof spec.playerClass !== 'string' || spec.playerClass === '') {
        throw new Error(`建筑 ${key} 缺少 playerClass`);
      }
      if (spec.cost === null || typeof spec.cost !== 'object' || Array.isArray(spec.cost)) {
        throw new Error(`建筑 ${key} 缺少成本表（资源名到数量的对象）`);
      }
      for (const [resource, amount] of Object.entries(spec.cost)) {
        if (!Number.isSafeInteger(amount) || amount < 0) {
          throw new Error(`建筑 ${key} 的成本资源 ${resource} 数量须为非负整数`);
        }
      }
      if (!Number.isSafeInteger(spec.hitsMax) || spec.hitsMax < 0) {
        throw new Error(`建筑 ${key} 的 hitsMax 须为非负整数`);
      }
      if (spec.pathing !== 'obstacle' && spec.pathing !== 'plain' && spec.pathing !== 'road') {
        throw new Error(`建筑 ${key} 的 pathing 须为 obstacle/plain/road`);
      }
      if (spec.capacity !== undefined
        && (!Number.isSafeInteger(spec.capacity) || spec.capacity < 0)) {
        throw new Error(`建筑 ${key} 的 capacity 须为非负整数`);
      }
      if (spec.production !== undefined && spec.production !== null) {
        const p = spec.production;
        if (p === null || typeof p !== 'object' || Array.isArray(p)) {
          throw new Error(`建筑 ${key} 的生产配方须为对象`);
        }
        for (const member of Object.keys(p)) {
          if (member !== 'interval' && member !== 'inputs' && member !== 'outputs') {
            throw new Error(`建筑 ${key} 的生产配方声明了未知选项 ${member}`);
          }
        }
        if (!Number.isSafeInteger(p.interval) || p.interval <= 0) {
          throw new Error(`建筑 ${key} 的生产周期须为正整数`);
        }
        for (const table of ['inputs', 'outputs']) {
          if (p[table] === undefined) continue;
          if (p[table] === null || typeof p[table] !== 'object' || Array.isArray(p[table])) {
            throw new Error(`建筑 ${key} 的生产${table === 'inputs' ? '输入' : '输出'}须为对象`);
          }
          for (const [resource, amount] of Object.entries(p[table])) {
            if (!Number.isSafeInteger(amount) || amount < 0) {
              throw new Error(`建筑 ${key} 的生产${table === 'inputs' ? '输入' : '输出'}资源 ${resource} 数量须为非负整数`);
            }
          }
        }
      }
      const fields = [];
      if (spec.fields !== undefined) {
        if (spec.fields === null || typeof spec.fields !== 'object' || Array.isArray(spec.fields)) {
          throw new Error(`建筑 ${key} 的字段表须为对象`);
        }
        for (const [name, value] of Object.entries(spec.fields)) {
          if (value !== null && typeof value === 'object' && value.visibility !== undefined
            && value.visibility !== 'public') {
            throw new Error(`建筑 ${key} 的字段 ${name} 可见性首片只支持 public`);
          }
          fields.push(fieldOf(namespace, name, value));
        }
      }
      let tick;
      if (spec.tick !== undefined) {
        tick = { slot: key + '.tick', process: key + '.tick.process' };
        requireFunction(namespace, tick.process, spec.tick);
      }
      declare('building', {
        key,
        playerClass: spec.playerClass,
        cost: spec.cost,
        hitsMax: spec.hitsMax,
        pathing: spec.pathing,
        capacity: spec.capacity,
        production: spec.production,
        fields,
        tick,
      });
      return key;
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
    resources,
    buildings,
    mounts,
  };
}

export function seal() {
  sealState();
  return Array.from(bindings.keys());
}

