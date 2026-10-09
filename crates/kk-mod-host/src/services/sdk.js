// 后台域的注册表与生命周期只存在于独立 VM，不进入玩法检查点。
const ops = Deno.core.ops;
const definitions = new Map();
let sealed = false;
let registrationFault = null;
function reject(message) {
  registrationFault ??= message;
  throw Error(message);
}
function immutable(value) {
  if (value && typeof value === 'object') {
    for (const child of Object.values(value)) immutable(child);
    Object.freeze(value);
  }
  return value;
}
export function createApi(namespace, config) {
  return Object.freeze({
    config: immutable(config),
    services: Object.freeze({
      define(key, callbacks) {
        if (sealed) reject('后台注册窗口已关闭');
        if (
          typeof key !== 'string' ||
          !key.startsWith(`${namespace}:`) ||
          !/^[a-zA-Z0-9_.-]+:[a-zA-Z0-9_.-]+$/.test(key)
        ) {
          reject('服务键须属于当前包命名空间');
        }
        if (definitions.has(key)) reject(`重复服务 ${key}`);
        if (!callbacks || typeof callbacks !== 'object' || Array.isArray(callbacks))
          reject('服务定义须为对象');
        for (const name of Object.keys(callbacks)) {
          if (!['start', 'request', 'stop'].includes(name) || typeof callbacks[name] !== 'function')
            reject(`非法服务回调 ${name}`);
        }
        if (typeof callbacks.request !== 'function') reject('服务须声明 request');
        ops.op_service_register(key);
        definitions.set(key, { namespace, callbacks: Object.freeze({ ...callbacks }) });
      },
    }),
  });
}
export function seal() {
  if (registrationFault) throw Error(registrationFault);
  sealed = true;
  return [...definitions.keys()];
}
export async function dispatch(key, phase, args, view, token) {
  const definition = definitions.get(key);
  if (!definition || !['start', 'request', 'stop'].includes(phase))
    throw Error('未知服务或生命周期');
  const context = Object.freeze({
    view: immutable(view),
    probe(name) {
      return ops.op_service_probe(token, name);
    },
  });
  const callback = definition.callbacks[phase];
  const result = callback ? ((await callback(context, immutable(args))) ?? null) : null;
  if (registrationFault) throw Error(registrationFault);
  return result;
}
