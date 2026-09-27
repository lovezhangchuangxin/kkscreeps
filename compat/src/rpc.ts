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

class EmbedClient {
  private child: ChildProcess | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (r: RpcOk) => void; reject: (e: Error) => void }>();

  private ensure(): ChildProcess {
    if (this.child && this.child.exitCode === null) return this.child;
    const bin = locateBinary();
    const child = spawn(bin, ['embed'], { stdio: ['pipe', 'pipe', 'inherit'] });
    // 子进程不能把宿主事件循环挂住：unref 后由进程退出钩子负责回收
    child.unref();
    child.stdout?.unref();
    child.stdin?.unref();
    child.on('error', (e) => this.failAll(e));
    child.on('exit', () => this.failAll(new Error('kkscreeps embed 子进程退出')));
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
    if (msg.ok) waiter.resolve(msg);
    else waiter.reject(new Error(msg.error));
  }

  private failAll(e: Error): void {
    const pending = [...this.pending.values()];
    this.pending.clear();
    for (const w of pending) w.reject(e);
  }

  call<T = unknown>(method: string, params: unknown): Promise<T> {
    const child = this.ensure();
    const id = this.nextId++;
    const stdin = child.stdin;
    if (!stdin) throw new Error('子进程 stdin 不可用');
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: (r) => resolve(r.result as T), reject });
      stdin.write(JSON.stringify({ id, method, params }) + '\n', (err) => {
        if (err) {
          this.pending.delete(id);
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
  }
}

const globalForClient = globalThis as typeof globalThis & { __kkEmbedClient?: EmbedClient };

export function client(): EmbedClient {
  if (!globalForClient.__kkEmbedClient) {
    const c = new EmbedClient();
    globalForClient.__kkEmbedClient = c;
    // 进程退出时兜底回收，防止 vitest worker 直接退出留下孤儿进程
    const cleanup = () => c.kill();
    process.once('exit', cleanup);
    process.once('SIGTERM', cleanup);
    process.once('SIGINT', cleanup);
  }
  return globalForClient.__kkEmbedClient;
}
