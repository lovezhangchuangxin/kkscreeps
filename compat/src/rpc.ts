// 管理 kkscreeps embed 子进程，将逐行 JSON 响应按请求 id 分发给等待者。
//
// 消息使用自定义 ok/error 字段，不是标准 JSON-RPC 2.0 格式；
// 协议以 crates/kk-cli/src/embed_main.rs 为准，设计依据见 ADR-010。
// client() 在同一 Node 全局中复用一个客户端；超时会终止整个连接并拒绝所有等待请求。
// 新连接不会恢复旧世界，调用方需重新 createShard。

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

/** 优先使用 KKSCREEPS_BIN，否则取 release、debug 中首个存在的文件；执行权限由 spawn 检查。 */
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

/** 请求超时毫秒数；环境变量仅接受有限正数，否则使用 60 秒。 */
function rpcTimeoutMs(): number {
  const v = Number(process.env['KKSCREEPS_RPC_TIMEOUT_MS']);
  return Number.isFinite(v) && v > 0 ? v : 60_000;
}

export class EmbedClient {
  private child: ChildProcess | null = null;
  private nextId = 1;
  private pending = new Map<number, PendingWaiter>();

  private ensure(): ChildProcess {
    // 退出事件清空当前连接；下一次请求会启动一个没有旧世界状态的新进程。
    if (this.child && this.child.exitCode === null) return this.child;
    const bin = locateBinary();
    const child = spawn(bin, ['embed'], { stdio: ['pipe', 'pipe', 'inherit'] });
    // 空闲连接不保持 Node 事件循环；通过 client() 创建时另注册退出清理钩子。
    child.unref();
    interface Unrefable {
      unref?(): void;
    }
    (child.stdout as unknown as Unrefable).unref?.();
    (child.stdin as unknown as Unrefable).unref?.();
    child.on('error', (e) => {
      // 旧连接的延迟事件不能清空已经启动的新连接及其等待请求。
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
    // 请求超时后终止整个子进程，防止没有响应的连接一直阻塞测试。
    // 同一连接的其他等待请求也会失败；后续请求重新启动进程。
    const timeoutMs = rpcTimeoutMs();
    return new Promise<T>((resolve, reject) => {
      // 响应与退出事件在当前同步代码结束后处理，届时 timer 已赋值。
      const waiter = {
        resolve: (r: RpcOk) => resolve(r.result as T),
        reject,
      } as PendingWaiter;
      this.pending.set(id, waiter);
      waiter.timer = setTimeout(() => {
        if (!this.pending.delete(id)) return;
        reject(new Error(`RPC 超时（${timeoutMs}ms）: ${method}`));
        // 只终止此请求所属的当前连接，不能影响后来启动的新连接。
        if (this.child === child) this.kill();
      }, timeoutMs);
      // 保留计时器对事件循环的引用，确保独立 Node 脚本等待响应。
      // 子进程与管道均已 unref；若计时器也 unref，顶层 await 可能以 exit 13 提前退出。
      // 收到响应、连接失效或写入失败时清除计时器，空闲连接不会阻止退出。
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
    // 清空连接后，旧进程的 exit 事件会被忽略；在这里立即拒绝全部等待请求。
    this.failAll(new Error('kkscreeps embed sidecar 被 kill（超时处置或进程退出）'));
  }
}

const globalForClient = globalThis as typeof globalThis & { __kkEmbedClient?: EmbedClient };

export function client(): EmbedClient {
  if (!globalForClient.__kkEmbedClient) {
    const c = new EmbedClient();
    globalForClient.__kkEmbedClient = c;
    // 回收共享子进程，避免 Node 退出后仍留下 embed 进程。
    // 注册信号监听会取消 Node 的默认退出行为；once 监听清理后重发信号以继续退出。
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
