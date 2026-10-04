// 链驱动器实验：整条贡献链在一次宿主调用内跑完，wrap→wrap 不经 Rust 回入。
//
// 与 spikes/deno-spike/examples/js_mod_host.rs 的差异：那里 next() 的载荷是
// 传给 Rust native 的 JS 回调（Rust 是链的驱动方）。本文件反过来——链由 JS
// 驱动，宿主只提供链的描述数据，wrap 之间靠 JS 闭包续行，Rust native 中心
// 是链内一次普通 op 调用。因此全程不需要 reentrant op。

const handlers = new Map();

function register(key, fn, kind) {
  handlers.set(key, { fn: fn, kind: kind });
}

// 跨 JS/Rust 边界的统一轨迹，便于 Rust 侧断言全链顺序。
function trace(step) {
  Deno.core.ops.op_chain_log(step);
}

// chain = { before: [key], wraps: [key], center: [kind, key?], after: [key] }
// depth 是当前 wrap 下标；到达 wraps 长度即进入中心。
//
// 返回 {consumed, value}：consumed 表示是否真的抵达中心。
// `after` 的时点是个陷阱——它属于**外层**返回而非递归基例：引擎语义是
// after 在整条 wrap 链退栈后执行（见 kk-engine contrib::run_wrapped），
// 若挂在基例上会变成「中心跑完立刻 after、wrap 尚未退栈」。
function runChain(chain, depth, ctx) {
  if (depth < chain.wraps.length) {
    const entry = handlers.get(chain.wraps[depth]);
    if (!entry) {
      throw new Error('未注册的 wrap 处理器 ' + chain.wraps[depth]);
    }
    let consumed = false;
    const value = entry.fn(ctx, function next() {
      if (consumed) {
        throw new Error('next() 至多调用一次');
      }
      consumed = true;
      return runChain(chain, depth + 1, ctx);
    });
    // 未调 next 即取消内层：中心与 after 一并不执行。
    return { consumed: consumed, value: value };
  }
  if (chain.center[0] === 'native') {
    Deno.core.ops.op_chain_native_center(ctx.id);
  } else {
    const entry = handlers.get(chain.center[1]);
    if (!entry) throw new Error('未注册的中心处理器 ' + chain.center[1]);
    entry.fn(ctx);
  }
  return { consumed: true, value: null };
}

function runSlot(chainJson, ctx) {
  const chain = JSON.parse(chainJson);
  for (let i = 0; i < chain.before.length; i++) {
    handlers.get(chain.before[i]).fn(ctx);
  }
  const result = runChain(chain, 0, ctx);
  // 只有真的抵达中心（未被取消）才跑 after，且在 wrap 全部退栈之后。
  if (result.consumed) {
    for (let i = 0; i < chain.after.length; i++) {
      handlers.get(chain.after[i]).fn(ctx);
    }
  }
  return result.value;
}
