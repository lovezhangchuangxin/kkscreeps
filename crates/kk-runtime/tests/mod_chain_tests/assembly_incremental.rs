//! P2 增量装配回归：KKPV 通道多拍下，干净行跳过/值行格位迁移/新对象入
//! 桶/消亡出表/跨房移动的读面与全量路径（JSON 通道同断言）一致。
//! 无 Mod 包夹具——Mod 玩家面在场时增量路径整体关闭。
use super::*;
use harness::creep_at;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[
        {"name":"W1N1","owner":"p1","rcl":2},{"name":"W0N1"}]}))?;
    Ok(e)
}

/// 同房移动的格位迁移（moveCell）与 createFlag 新对象入桶（重建路径）：
/// look 空间索引、find、分类表与 getObjectById 都必须读当拍值。
#[test]
fn incremental_assembly_moves_cells_and_admits_new_members() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        creep_at(&mut e, "W1N1", "m1", "p1", 10, 10, &["move"])?;
        let first = e
            .run_player(
                "p1",
                "[Game.rooms.W1N1.lookAt(10,10).some(o=>o.type===LOOK_CREEPS),\
                 Game.rooms.W1N1.lookForAt(LOOK_CREEPS,11,10).length,\
                 Game.rooms.W1N1.find(FIND_CREEPS).length,\
                 Game.getObjectById(Game.rooms.W1N1.find(FIND_CREEPS)[0].id)!==null]",
            )?
            .expr["value"]
            .clone();
        assert_eq!(first, json!([true, 0, 1, true]), "首拍基线（{binary}）");
        // 移动后：旧格清空、新格命中（格位迁移不走房重建，索引必须同步）。
        e.run_player("p1", "Game.creeps.m1.move(RIGHT)")?;
        let moved = e
            .run_player(
                "p1",
                "[Game.rooms.W1N1.lookAt(11,10).some(o=>o.type===LOOK_CREEPS),\
                 Game.rooms.W1N1.lookAt(10,10).some(o=>o.type===LOOK_CREEPS),\
                 Game.rooms.W1N1.lookForAt(LOOK_CREEPS,10,10).length,\
                 Game.creeps.m1.pos.x]",
            )?
            .expr["value"]
            .clone();
        assert_eq!(
            moved,
            json!([true, false, 0, 11]),
            "移动后格位迁移（{binary}）"
        );
        // 新对象（旗）走桶整建：look/find/分类同时可见，键序含新键。
        e.run_player("p1", "Game.rooms.W1N1.createFlag(20,20,'F1')")?;
        let flagged = e
            .run_player(
                "p1",
                "[Game.flags.F1!==undefined,\
                 Game.rooms.W1N1.lookForAt(LOOK_FLAGS,20,20).length,\
                 Game.rooms.W1N1.find(FIND_FLAGS).length,\
                 Object.keys(Game.flags).join(','),\
                 Game.flags.F1.pos.roomName]",
            )?
            .expr["value"]
            .clone();
        assert_eq!(
            flagged,
            json!([true, 1, 1, "F1", "W1N1"]),
            "新旗入桶（{binary}）"
        );
    }
    Ok(())
}

/// 消亡（临时门到期）与跨房移动（边缘自动过渡）：deadIds 出表与两房桶
/// 重建后，getObjectById/find/room 挂载必须与全量路径一致。
#[test]
fn incremental_assembly_removes_dead_and_moves_across_rooms() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let temporary = e.place_object(
            "W1N1",
            "portal",
            &json!({"pos":[25,25],"destination":{"room":"W0N1","x":30,"y":30},"ticksToDecay":3}),
        )?;
        // 动态找东缘可通行行（地形随种子生成，不假设 48/49 列固定可走）。
        let lane = e
            .run_player(
                "p1",
                "(function(){for(let y=10;y<40;y++){\
                   if(Game.map.getTerrainAt(48,y,'W1N1')!=='wall'&&Game.map.getTerrainAt(49,y,'W1N1')!=='wall')return y;}\
                 return -1;})()",
            )?
            .expr["value"]
            .as_i64()
            .unwrap();
        assert!(lane >= 0, "夹具应存在可通行边缘行（{binary}）");
        let m2 = e.place_object(
            "W1N1",
            "creep",
            &json!({"pos":[48,lane],"owner":"p1","body":["move","move"],"name":"m2"}),
        )?;
        // 移上边缘格：同 tick 迁入邻房（引擎边缘过渡语义）。
        e.run_player("p1", "Game.creeps.m2.move(RIGHT)")?;
        let crossed = e
            .run_player(
                "p1",
                &format!(
    "[Game.getObjectById('{m2}').room!==undefined?Game.getObjectById('{m2}').room.name:null,\
                     Game.rooms.W0N1!==undefined?Game.rooms.W0N1.find(FIND_CREEPS).length:-1,\
                     Game.rooms.W1N1.find(FIND_CREEPS).length,\
                     Game.rooms.W1N1.lookForAt(LOOK_CREEPS,49,{lane}).length]"
                ),
            )?
            .expr["value"]
            .clone();
        assert_eq!(crossed, json!(["W0N1", 1, 0, 0]), "跨房桶迁移（{binary}）");
        // 临时门倒数到期移除（deadIds）：对象读面 null、find 不再命中。
        let mut gone = json!(null);
        for _ in 0..6 {
            gone = e
                .run_player(
                    "p1",
                    &format!(
                        "Game.getObjectById('{temporary}')!==null && Game.getObjectById('{temporary}').structureType==='portal'",
                    ),
                )?
                .expr["value"]
                .clone();
            if gone == json!(false) {
                break;
            }
        }
        assert_eq!(gone, json!(false), "临时门到期后出表（{binary}）");
        let after = e
            .run_player(
                "p1",
                &format!(
                    "[Game.getObjectById('{}'),\
                     Game.rooms.W1N1.find(FIND_STRUCTURES).filter(s=>s.structureType==='portal').length,\
                     Game.rooms.W1N1.lookForAt(LOOK_STRUCTURES,25,25).length]",
                    temporary
                ),
            )?
            .expr["value"]
            .clone();
        assert_eq!(after, json!([null, 0, 0]), "消亡对象全读面清净（{binary}）");
    }
    Ok(())
}
