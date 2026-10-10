//! wrapper 常驻的字段记忆拍级失效回归：wrapper 级共享纪元戳在多字段
//! 访问下必须整包清空 `_` 前缀记忆（各字段只清自己会漏清——首个访问
//! 字段推进共享戳后，后续字段看到已当前的戳而返回上一拍记忆）。
use super::*;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[
        {"name":"W1N1","owner":"p1","rcl":2},{"name":"W2N1"}]}))?;
    Ok(e)
}

#[test]
fn wrapper_residency_multi_field_memo_invalidation() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = e.place_object(
            "W1N1",
            "invaderCore",
            &json!({"pos":[25,25],"level":3,"ticksToDeploy":20}),
        )?;
        // level 先于 ticksToDeploy 访问：若无整包清空，第二拍 level 推进
        // wrapper 戳后 ticksToDeploy 会带着 20 的旧记忆漏过失效。
        let code = format!(
            "const s=Game.getObjectById('{}');[s.level,s.ticksToDeploy,s instanceof StructureInvaderCore,Game.structures[s.id]===undefined]",
            id
        );
        assert_eq!(
            e.run_player("p1", &code)?.expr["value"],
            json!([3, 20, true, true])
        );
        assert_eq!(
            e.run_player("p1", &code)?.expr["value"],
            json!([3, 19, true, true]),
            "第二拍：多字段记忆须一并失效（ticksToDeploy 折算绝对键）"
        );
        assert_eq!(
            e.run_player("p1", &code)?.expr["value"],
            json!([3, 18, true, true])
        );
    }
    Ok(())
}

#[test]
fn wrapper_residency_identity_and_stale_reference_reads_live_data() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let id = e.place_object(
            "W1N1",
            "invaderCore",
            &json!({"pos":[25,25],"level":3,"ticksToDeploy":20}),
        )?;
        // 第一拍持有 wrapper 引用跨拍再读：同一性保持（常驻观察面），
        // 且经 rawData 回查得到当拍真值——旧行为每拍重建包装，引用是
        // 分离实例但 getter 同样回查；两形态的读值必须一致。
        let code = format!(
            "const s=Game.getObjectById('{}');globalThis.held=s;[s.ticksToDeploy,globalThis.held===s]",
            id
        );
        assert_eq!(e.run_player("p1", &code)?.expr["value"], json!([20, true]));
        let code2 = format!(
            "const s=Game.getObjectById('{}');[s.ticksToDeploy,globalThis.held===s,globalThis.held.ticksToDeploy]",
            id
        );
        assert_eq!(
            e.run_player("p1", &code2)?.expr["value"],
            json!([19, true, 19]),
            "跨拍持有的引用与当拍包装同体且读当拍值"
        );
    }
    Ok(())
}
