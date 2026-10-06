import { sealed, bindings, declines } from './state.js';
import { refreshContext } from './refresh.js';
import { withStreamScope, streamDomain } from './determinism.js';
import { createResources } from './resources.js';
import { createObjects } from './objects.js';
import { createMovement } from './movement.js';
import { createCombat } from './combat.js';
import { createBuild } from './build.js';

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
// 房间级移动竞争槽的 ctx.contest 读写本次输入中竞争者的优先级数值
// （getPriority/setPriority），只改变仲裁排序，不开放请求或获准集合。
// 房间级移动准入槽的 ctx.admission 读写本次输入中执行者的准入终值
// （getAdmitted/setAdmitted），初值即原生 can_move 结果，终值同时约束
// 链取消与执行阶段；evaluate 按候选当前终值只读预演取消链，返回会
// 存活的胜者 ID（宿主同一取消链代码；按调用时点即时计算，非快照，
// 障碍槽尚未追加、结果为上界）。房间级移动障碍槽的 ctx.obstacle 提供单向追加阻挡
// （isBlocked/block），不能放行 native 判定的阻挡。房间级移动预检槽
// 的 ctx.precheck 同为单向收紧（isBlocked/block），被否决的移动请求
// 等同从未登记、不参与竞争。均不可枚举、不可替换且只在处理器同步期
// 内有效。
// 处理器的持久状态只准走 ctx.state——模块全局不随失败候选回滚，
// 用它做累计会破坏同输入重试的确定性。
export function dispatch(key, ctx) {
  if (!sealed) throw new Error('dispatch 仅在注册密封后可用');
  const entry = bindings.get(key);
  if (!entry || typeof entry.fn !== 'function') {
    throw new Error(`绑定 ${JSON.stringify(key)} 没有注册函数`);
  }
  if (entry.kind === 'wrap') throw new Error('wrap 必须经整槽执行入口调用');
  // 处理器执行期建立随机流域与确定性时钟作用域；直接派发路径的 ctx
  // 可能缺少 tick（测试直呼），按注册期口径 0 对齐。
  const result = withStreamScope(
    { domain: streamDomain(key, ctx), tick: ctx?.tick ?? 0 },
    () => entry.fn(ctx),
  );
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
  const { resources } = createResources({ ops, token, fault, write });
  const { objects } = createObjects({ ops, token, fault, write });
  const { movement, contest, admission, precheck, pull, cost, obstacle } = createMovement({ ops, token, fault, write });
  const { combat, incoming } = createCombat({ ops, token, fault, write });
  const { build } = createBuild({ ops, token, fault, write });
  function simple(key) {
    const ctx = ops.op_mod_chain_read(token, key);
    let open = true;
    Object.defineProperty(ctx, 'resources', { value: resources(key, ctx, () => open) });
    Object.defineProperty(ctx, 'objects', { value: objects(key, ctx, () => open) });
    Object.defineProperty(ctx, 'movement', { value: movement(key, () => open) });
    Object.defineProperty(ctx, 'combat', { value: combat(key, () => open) });
    Object.defineProperty(ctx, 'build', { value: build(key, ctx, () => open) });
    Object.defineProperty(ctx, 'incoming', { value: incoming(key, () => open) });
    Object.defineProperty(ctx, 'contest', { value: contest(key, () => open) });
    Object.defineProperty(ctx, 'admission', { value: admission(key, () => open) });
    Object.defineProperty(ctx, 'obstacle', { value: obstacle(key, () => open) });
    Object.defineProperty(ctx, 'precheck', { value: precheck(key, () => open) });
    Object.defineProperty(ctx, 'pull', { value: pull(key, () => open) });
    Object.defineProperty(ctx, 'cost', { value: cost(key, () => open) });
    try {
      checkResult(
        key,
        withStreamScope({ domain: streamDomain(key, ctx), tick: ctx.tick }, () =>
          bindings.get(key).fn(ctx),
        ),
      );
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
    Object.defineProperty(ctx, 'incoming', { value: incoming(key, () => open) });
    Object.defineProperty(ctx, 'contest', { value: contest(key, () => open) });
    Object.defineProperty(ctx, 'admission', { value: admission(key, () => open) });
    Object.defineProperty(ctx, 'obstacle', { value: obstacle(key, () => open) });
    Object.defineProperty(ctx, 'precheck', { value: precheck(key, () => open) });
    Object.defineProperty(ctx, 'pull', { value: pull(key, () => open) });
    Object.defineProperty(ctx, 'cost', { value: cost(key, () => open) });
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
      checkResult(
        key,
        withStreamScope({ domain: streamDomain(key, ctx), tick: ctx.tick }, () =>
          bindings.get(key).fn(ctx, next),
        ),
      );
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
