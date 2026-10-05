# JS 类型生成夹具

两个互不依赖的普通包共用同一包根目录，供 `kkscreeps mod types` 与选择门验收：alpha 声明覆盖全部 ValueKind 的房间可见对象 `alpha:beacon`（含 public/owner/server-only 三种字段可见性、enum/array/map/ref 字段与归属字段）、对象域意图 `alpha:salute`、资源 `alpha:fuel` 与建筑 `alpha:gate`；beta 声明归属者集合可见对象 `beta:relay`（owner-collection 须声明归属字段）与可空参数意图 `beta:ping`（args nullable → 声明为可选参数）、资源 `beta:spark`。

只选 alpha 时 beta 完全不进入计划（声明无 beta: 残留），反之亦然；两包合并生成时命名空间互不串扰。生成的 interface 把投影信封键（id/room/x/y/pos/type/modType）声明一次，同名声明字段（位置字段）不重复出现；enum 联合按集合序输出，与声明处书写顺序无关。声明文件必须放在包根目录之外（包内全部文件字节计入计划指纹）。

```bash
cargo build -p kk-cli
target/debug/kkscreeps mod types --root compat/fixtures/mod-typed --out /tmp/mod-typed.d.ts --report /tmp/mod-typed.json
target/debug/kkscreeps mod types --root compat/fixtures/mod-typed --select alpha --out /tmp/mod-typed-alpha.d.ts --report /tmp/mod-typed-alpha.json
target/debug/kkscreeps mod types --root compat/fixtures/mod-typed --out /tmp/mod-typed.d.ts --check --report /tmp/mod-typed-check.json   # 声明过期或缺失时退出 2
```

生成器契约见[玩家类型生成](../../../docs/architecture/mod-types.md)。
