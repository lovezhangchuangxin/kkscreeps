# fixtures/

回放夹具：`{room, ticks:[RoomSnapshot, ...RoomDelta[]]}`（@kkscreeps/shared
feed 契约 v0；首元素恒为快照）。由 kk-cli 只读子命令 `export-fixtures`
（四张脚本世界：plains 全平原 / standard 标准房 / swamp-walls 沼泽墙混合 /
dense-edge 密集边界，settle_tests Fixture 同款摆法）跑 30 tick 逐 tick 落
world_json，再经 `src/feed/differ.ts` 转换成本契约形。

再生（确定性链，产物逐字节一致）：

    cargo build -p kk-cli
    node scripts/build-fixtures.mjs            # 在 packages/renderer/ 下

固定 seed + 固定 `TickInput` 墙钟注入（ADR-013 决策 5），引擎空意图推进
（零 V8，与 embed/测试同一条 settle 链）。夹具与契约的形状一致性由
test/fixtures.test.ts 断言。
