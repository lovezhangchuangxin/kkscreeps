//! InterShardMemory 本地段与 CPU shardLimits 域集成（套件 29.3/29.4 同形）：
//! ISM null 起点/同拍 round-trip/非字符串与超限拒绝不留痕（套件之外另钉
//! 跨拍持久与冷恢复——官方后端专有 API，OSS 无实现，语义按套件契约）；
//! shardLimits 读面（单 shard 恒 {shard0: limit}、和==limit）与
//! setShardLimits 四错误分支（和不等/未知 shard/负值/非整数→ERR_INVALID_ARGS）。
use super::*;
use std::collections::BTreeMap;

const ROOM: &str = "W1N1";

fn engine_on(binary: bool) -> EmbedEngine {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e
}

fn owned_world() -> Value {
    json!({ "players": ["p1"], "rooms": [{ "name": ROOM, "rcl": 1, "owner": "p1" }] })
}

/// ISM-001/002 同形：null 起点与同拍 round-trip；另钉跨拍持久（setLocal
/// 拍之后的独立拍仍可读，且经检查点冷恢复保留——PlayerState.ism_local
/// 的保存恢复链）。
#[test]
fn ism_local_roundtrip_and_persistence() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        e.tick(1)?;
        let initial = e.run_player("p1", "InterShardMemory.getLocal()")?;
        assert_eq!(initial.expr["value"], json!(null), "起点 null");

        let got = e.run_player(
            "p1",
            "InterShardMemory.setLocal('hello shard0'); InterShardMemory.getLocal()",
        )?;
        assert_eq!(got.expr["value"], json!("hello shard0"), "同拍 round-trip");

        // 跨拍：新拍无 setLocal 仍读到持久值（写回候选已提交）。
        let later = e.run_player("p1", "InterShardMemory.getLocal()")?;
        assert_eq!(later.expr["value"], json!("hello shard0"), "跨拍保留");

        // 写回失败轮（Memory 不可序列化）ISM 候选随整轮丢弃，不泄漏。
        e.run_player("p1", "InterShardMemory.setLocal('persisted')")?;
        let failed = e.run_player(
            "p1",
            "InterShardMemory.setLocal('leak'); Memory.big = 1n; null",
        )?;
        assert_eq!(failed.expr["ok"], json!(false), "写回失败轮");
        let kept = e.run_player("p1", "InterShardMemory.getLocal()")?;
        assert_eq!(kept.expr["value"], json!("persisted"), "失败轮候选不泄漏");

        // 冷恢复：检查点往返后段落仍在。
        let checkpoint = e.checkpoint(json!({"case": "ism"}))?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(checkpoint, None)?;
        let after = restored.run_player("p1", "InterShardMemory.getLocal()")?;
        // 检查点时刻的现值为 'persisted'（丢弃腿的基线写入成功、leak 被丢）。
        assert_eq!(after.expr["value"], json!("persisted"), "冷恢复保留");
    }
    Ok(())
}

/// ISM-003/004 同形：非字符串与超 100 KiB 拒绝且不留痕（抛错语义；segment
/// 保持种子值）。
#[test]
fn ism_local_rejects_non_string_and_oversize() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        e.tick(1)?;
        let result = e.run_player(
            "p1",
            "function probe(v) { \
                try { InterShardMemory.setLocal(v); return {kind:'ok', after: InterShardMemory.getLocal()}; } \
                catch (err) { return {kind:'reject'}; } \
            } \
            InterShardMemory.setLocal('seed'); \
            ({ \
                str: probe('payload'), \
                num: probe(42), \
                obj: probe({a:1}), \
                nul: probe(null), \
                undef: probe(undefined), \
                big: probe('x'.repeat(102401)), \
            })",
        )?;
        let v = &result.expr["value"];
        assert_eq!(v["str"]["kind"], json!("ok"));
        assert_eq!(v["str"]["after"], json!("payload"));
        for key in ["num", "obj", "nul", "undef", "big"] {
            assert_eq!(
                v[key]["kind"],
                json!("reject"),
                "{key} 须抛错拒绝（不留痕语义由 reject 保证）"
            );
        }
        // 超限后 segment 仍持种子之后的合法值（big 拒绝不动 str 的 payload）。
        let after = e.run_player("p1", "InterShardMemory.getLocal()")?;
        assert_eq!(after.expr["value"], json!("payload"), "拒绝不留痕");
        // 恰好 100 KiB（102400 码元）不超限——严格大于门。
        let exact = e.run_player(
            "p1",
            "try { InterShardMemory.setLocal('y'.repeat(102400)); 'ok'; } catch (e) { 'reject'; }",
        )?;
        assert_eq!(exact.expr["value"], json!("ok"), "恰好 100 KiB 成功");
    }
    Ok(())
}

/// 双玩家隔离：各自 setLocal/getLocal 互不串扰（注入按玩家解算、每玩家
/// 独立沙箱闭包、写回按 handle 键控——三层结构性保证的行为级锁）。
#[test]
fn ism_local_is_per_player() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&json!({ "players": ["p1", "p2"], "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "W2N1", "rcl": 1, "owner": "p2" },
        ]}))?;
        e.tick(1)?;
        e.run_players(&BTreeMap::from([
            (
                "p1".to_string(),
                "InterShardMemory.setLocal('from-p1'); null".to_string(),
            ),
            (
                "p2".to_string(),
                "InterShardMemory.setLocal('from-p2'); null".to_string(),
            ),
        ]))?;
        let reads = e.run_players(&BTreeMap::from([
            ("p1".to_string(), "InterShardMemory.getLocal()".to_string()),
            ("p2".to_string(), "InterShardMemory.getLocal()".to_string()),
        ]))?;
        let v1 = &reads.get("p1").expect("p1 回执").expr["value"];
        let v2 = &reads.get("p2").expect("p2 回执").expr["value"];
        assert_eq!(v1, &json!("from-p1"), "p1 读到自己的段落");
        assert_eq!(v2, &json!("from-p2"), "p2 读到自己的段落");
    }
    Ok(())
}

/// CPU-SHARD-001/002 同形：读面为正整数映射且和==Game.cpu.limit。
#[test]
fn cpu_shard_limits_read_surface() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        e.tick(1)?;
        let result = e.run_player(
            "p1",
            "const l = Game.cpu.shardLimits; \
             ({ keys: Object.keys(l), values: Object.values(l), sum: Object.values(l).reduce((a,b)=>a+b,0), limit: Game.cpu.limit })",
        )?;
        let v = &result.expr["value"];
        assert_eq!(v["keys"], json!(["shard0"]));
        assert_eq!(v["values"].as_array().unwrap().len(), 1);
        assert!(v["values"][0].as_i64().unwrap_or(-1) >= 0, "非负整数配额");
        assert_eq!(v["sum"], v["limit"], "各 shard 配额之和==每日限额");
        // getter 每次新对象：改返回值不影响后续读。
        let mutated = e.run_player(
            "p1",
            "const a = Game.cpu.shardLimits; a.shard0 = 999; Game.cpu.shardLimits.shard0 === a.shard0",
        )?;
        assert_eq!(mutated.expr["value"], json!(false), "读面 getter 新对象");
    }
    Ok(())
}

/// CPU-SHARD-003 同形：四类非法输入均 ERR_INVALID_ARGS；合法分配（唯一
/// 形态 {shard0: limit}）返回 OK。
#[test]
fn cpu_set_shard_limits_error_branches() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        e.tick(1)?;
        let result = e.run_player(
            "p1",
            "const original = Object.assign({}, Game.cpu.shardLimits); \
             const names = Object.keys(original); \
             const sum = Object.values(original).reduce((a,b)=>a+b,0); \
             const sumMismatch = {}; names.forEach((s) => { sumMismatch[s] = original[s]; }); \
             sumMismatch[names[0]] = Math.max(0, sum - 1); \
             const unknown = Object.assign({}, original); \
             delete unknown[names[0]]; unknown['shard_does_not_exist_999'] = original[names[0]]; \
             const negative = Object.assign({}, original); negative[names[0]] = -1; \
             const nonInteger = Object.assign({}, original); nonInteger[names[0]] = (original[names[0]] || 0) + 0.5; \
             ({ \
                sumMismatch: Game.cpu.setShardLimits(sumMismatch), \
                unknown: Game.cpu.setShardLimits(unknown), \
                negative: Game.cpu.setShardLimits(negative), \
                nonInteger: Game.cpu.setShardLimits(nonInteger), \
                valid: Game.cpu.setShardLimits(original), \
             })",
        )?;
        let v = &result.expr["value"];
        assert_eq!(v["sumMismatch"], json!(-10));
        assert_eq!(v["unknown"], json!(-10));
        assert_eq!(v["negative"], json!(-10));
        assert_eq!(v["nonInteger"], json!(-10));
        assert_eq!(v["valid"], json!(0), "唯一合法分配返回 OK");
    }
    Ok(())
}
