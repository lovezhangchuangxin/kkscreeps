// `kkscreeps embed` sidecar 的 JSON-RPC 客户端。
// 协议定义以 Rust 侧为准（crates/kk-cli/src/embed_main.rs）：一行一条 JSON-RPC。
// 子进程在模块级单例持有、跨适配器实例复用（screeps-ok 每个测试都会 createAdapter），
// 进程退出时统一回收。

import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Readable } from 'node:stream';

export interface RpcRequest {
  id: number;
  method: string;
  params: unknown;
}

export interface RpcOk {
  id: number;
  ok: true;
  result: unknown;
}

export interface RpcErr {
  id: number;
  ok: false;
  error: string;
}

export type RpcResponse = RpcOk | RpcErr;

/** kkscreeps 二进制定位：env 优先，其次仓库 target/ 下的构建产物。 */
export function locateBinary(): string {
  const fromEnv = process.env['KKSCREEPS_BIN'];
  if (fromEnv) return fromEnv;
  // dist/rpc.js → compat/dist → 仓库根
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  const candidates = [
    resolve(repoRoot, 'target/release/kkscreeps'),
    resolve(repoRoot, 'target/debug/kkscreeps'),
  ];
  for (const c of candidates) {
    if (existsSync(c)) return c;
  }
  throw new Error(`找不到 kkscreeps 二进制（设置 KKSCREEPS_BIN 或构建 target/release|debug/kkscreeps）`);
}

interface PendingWaiter {
  resolve: (r: RpcOk) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

/** 调用超时：默认 60s，KKSCREEPS_RPC_TIMEOUT_MS 覆盖（CI 棘轮防卡死）。 */
function rpcTimeoutMs(): number {
  const v = Number(process.env['KKSCREEPS_RPC_TIMEOUT_MS']);
  return Number.isFinite(v) && v > 0 ? v : 60_000;
}

export class EmbedClient {
  private child: ChildProcess | null = null;
  private nextId = 1;
  private pending = new Map<number, PendingWaiter>();

  private ensure(): ChildProcess {
    // 存活判断用 exitCode：被信号杀死（OOM SIGKILL / 外部 kill）时 exitCode 保持 null
    // 但 exit 回调已把 this.child 置空——不会出现"死子进程被永久复用、RPC 全部
    // ERR_STREAM_DESTROYED 且永不重启"的状态
    if (this.child && this.child.exitCode === null) return this.child;
    const bin = locateBinary();
    const child = spawn(bin, ['embed'], { stdio: ['pipe', 'pipe', 'inherit'] });
    // 子进程不能把宿主事件循环挂住：unref 后由进程退出钩子负责回收
    child.unref();
    interface Unrefable {
      unref?(): void;
    }
    (child.stdout as unknown as Unrefable).unref?.();
    (child.stdin as unknown as Unrefable).unref?.();
    child.on('error', (e) => {
      // 陈旧子进程守卫：超时 kill 后旧 child 的回调可能在重 spawn 之后到达，
      // 不得误清新 child 与其 pending
      if (this.child !== child) return;
      this.child = null;
      this.failAll(e);
    });
    child.on('exit', () => {
      if (this.child !== child) return;
      this.child = null;
      this.failAll(new Error('kkscreeps embed 子进程退出'));
    });
    const stdout = child.stdout as Readable;
    let buf = '';
    stdout.setEncoding('utf8');
    stdout.on('data', (chunk: string) => {
      buf += chunk;
      let idx: number;
      while ((idx = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, idx);
        buf = buf.slice(idx + 1);
        if (!line.trim()) continue;
        this.dispatch(line);
      }
    });
    this.child = child;
    return child;
  }

  private dispatch(line: string): void {
    let msg: RpcResponse;
    try {
      msg = JSON.parse(line) as RpcResponse;
    } catch {
      return;
    }
    const waiter = this.pending.get(msg.id);
    if (!waiter) return;
    this.pending.delete(msg.id);
    clearTimeout(waiter.timer);
    if (msg.ok) waiter.resolve(msg);
    else waiter.reject(new Error(msg.error));
  }

  private failAll(e: Error): void {
    const pending = [...this.pending.values()];
    this.pending.clear();
    for (const w of pending) {
      clearTimeout(w.timer);
      w.reject(e);
    }
  }

  call<T = unknown>(method: string, params: unknown): Promise<T> {
    const child = this.ensure();
    const id = this.nextId++;
    const stdin = child.stdin;
    if (!stdin) throw new Error('子进程 stdin 不可用');
    // 调用超时（工具链-4 清偿）：sidecar 死锁时 pending 永不结算，会把上游
    // 测试进程挂死——超时拒绝并 kill 子进程（死锁的 sidecar 不可恢复，下一次
    // 调用会重新 spawn，即"外部 SIGTERM 可恢复"的内置化）
    const timeoutMs = rpcTimeoutMs();
    return new Promise<T>((resolve, reject) => {
      // timer 在 pending.set 之后赋值：dispatch/failAll 都在异步 IO 回调里，
      // 不会读到未赋值的 timer
      const waiter = {
        resolve: (r: RpcOk) => resolve(r.result as T),
        reject,
      } as PendingWaiter;
      this.pending.set(id, waiter);
      waiter.timer = setTimeout(() => {
        if (!this.pending.delete(id)) return;
        reject(new Error(`RPC 超时（${timeoutMs}ms）: ${method}`));
        // 死锁的 sidecar 不可恢复：kill 之，下一次调用重新 spawn。
        // 同一性校验：陈旧 child 上的孤儿 pending 各自超时时，不得误杀已重
        // spawn 的健康 child
        if (this.child === child) this.kill();
      }, timeoutMs);
      waiter.timer.unref?.();
      stdin.write(JSON.stringify({ id, method, params }) + '\n', (err) => {
        if (err && this.pending.delete(id)) {
          clearTimeout(waiter.timer);
          reject(err);
        }
      });
    });
  }

  /** 引擎侧 teardown（世界重置、沙箱回收）；子进程保留复用。 */
  async reset(): Promise<void> {
    await this.call('teardown', {});
  }

  kill(): void {
    if (this.child && this.child.exitCode === null) {
      this.child.kill('SIGTERM');
    }
    this.child = null;
    // 被杀 child 的 exit 事件受同一性守卫直接 return（不 failAll）——幸存
    // pending 若只等各自超时会以"RPC 超时"误归因，此处按真实死因立即结算
    this.failAll(new Error('kkscreeps embed sidecar 被 kill（超时处置或进程退出）'));
  }
}

const globalForClient = globalThis as typeof globalThis & { __kkEmbedClient?: EmbedClient };

export function client(): EmbedClient {
  if (!globalForClient.__kkEmbedClient) {
    const c = new EmbedClient();
    globalForClient.__kkEmbedClient = c;
    // 进程退出时兜底回收，防止 vitest worker 直接退出留下孤儿进程。
    // 信号处理：cleanup 后重发信号——注册 SIGTERM/SIGINT 监听会抑制 Node 默认终止
    // 行为，不重发的话有未决定时器时进程会挂住不退（Ctrl+C / timeout 命令场景）
    const cleanup = () => c.kill();
    process.once('exit', cleanup);
    for (const sig of ['SIGTERM', 'SIGINT'] as const) {
      process.once(sig, () => {
        cleanup();
        process.kill(process.pid, sig);
      });
    }
  }
  return globalForClient.__kkEmbedClient;
}
