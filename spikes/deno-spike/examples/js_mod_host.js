// Research fixture only. The mod behavior is ordinary JavaScript.
// Public production SDK, object schemas, permissions and storage are not built here.
(() => {
  const ops = Deno.core.ops;
  const hooks = new Map();
  let frozen = false;
  globalThis.register = (name, callback) => {
    if (frozen || hooks.has(name)) throw new Error('registration closed or duplicate');
    hooks.set(name, callback);
  };
  register('replace', (id) => ops.op_mod_set(id, 10));
  register('wrap', (id) => {
    // next() delegates to Rust; the native implementation calls a JS lifecycle
    // callback which itself reads/writes the same owned transaction through ops.
    const next = () =>
      ops.op_mod_native(id, () => {
        ops.op_mod_set(id, ops.op_mod_get(id) + 2);
        return ops.op_mod_get(id);
      });
    ops.op_mod_set(id, ops.op_mod_get(id) + 1);
    next();
    ops.op_mod_set(id, ops.op_mod_get(id) + 4);
  });
  register('fail', (id) => {
    globalThis.volatileAttempts = (globalThis.volatileAttempts || 0) + 1;
    ops.op_mod_set(id, 999);
    throw new Error('mod failed after write');
  });
  frozen = true;
  globalThis.runHook = (name, id) => hooks.get(name)(id);
  globalThis.fieldIncrement = (n) => {
    for (let i = 0; i < n; i++) ops.op_mod_set(i, ops.op_mod_get(i) + 1);
  };
  globalThis.batchIncrement = () => {
    const values = ops.op_mod_read_all();
    for (let i = 0; i < values.length; i++) values[i] += 1;
    ops.op_mod_write_all(values);
  };
})();
