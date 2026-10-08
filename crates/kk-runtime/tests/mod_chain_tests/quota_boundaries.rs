//! 可覆盖配额不能用 RCL8 单件上限推断房间实际对象数量。
use super::*;

#[test]
fn singleton_max_quota_blocks_inactive_spawn_even_for_external_intents() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(
            &[(
                "quota",
                package("api.rules.override('builtin:structure.quota.spawn.rcl8',1);"),
            )],
            binary,
        )?;
        let far = e.place_object(
            "W1N1",
            "spawn",
            &json!({"name":"far","pos":[40,20],"owner":"p1","store":{"energy":300}}),
        )?;
        e.place_object(
            "W1N1",
            "spawn",
            &json!({"name":"near","pos":[20,20],"owner":"p1","store":{"energy":300}}),
        )?;
        let run = e.run_player("p1", "Game.spawns.far.isActive()")?;
        assert_eq!(run.expr["value"], false);
        e.settle_external(kk_engine::intent::IntentBatch::from_json(
            "p1",
            &json!({far.clone():{"createCreep":{"body":["move"],"name":"blocked"}}}),
        ))?;
        assert!(
            e.get_object(&far).unwrap()["spawning"].is_null(),
            "失活 spawn 不得开启孵化"
        );
    }
    Ok(())
}

#[test]
fn singleton_max_quota_still_counts_existing_links() -> anyhow::Result<()> {
    for binary in [false, true] {
        for level_quota in [1, 2] {
            let source = package(&format!(
                "api.rules.override('builtin:structure.quota.link.rcl8',1);api.rules.override('builtin:structure.quota.link.rcl6',{level_quota});"
            ));
            let mut e = engine(&[("quota", source)], binary)?;
            let far = link(&mut e, 40, 100, 0)?;
            let near = link(&mut e, 20, 0, 0)?;
            let middle = link(&mut e, 30, 0, 0)?;
            e.run_player("p1", &format!(
                "const far=Game.getObjectById('{far}'),near=Game.getObjectById('{near}'),middle=Game.getObjectById('{middle}');Memory.active=[far.isActive(),near.isActive(),middle.isActive()];far.transferEnergy(near,50);"
            ))?;
            let memory: Value = serde_json::from_str(&e.player_state("p1").unwrap().raw_memory)?;
            assert_eq!(
                memory["active"],
                json!([false, true, level_quota == 2]),
                "{binary}/{level_quota}"
            );
            assert_eq!(
                e.get_object(&far).unwrap()["store"]["energy"],
                100,
                "失活 link 不得发送能量"
            );
            assert_eq!(e.get_object(&near).unwrap()["store"]["energy"], 0);
            // 外部指令绕过玩家预检，权威 link 门也必须拒绝失活发送端。
            e.settle_external(kk_engine::intent::IntentBatch::from_json(
                "p1",
                &json!({far.clone():{"transfer":{"id":near,"amount":50}}}),
            ))?;
            assert_eq!(e.get_object(&far).unwrap()["store"]["energy"], 100);
        }
    }
    Ok(())
}
