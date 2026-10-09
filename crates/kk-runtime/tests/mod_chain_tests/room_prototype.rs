//! Room 原型包装跨拍生效，私有查询上下文按接收者与房间隔离。
use super::*;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1"],"rooms":[
        {"name":"W1N1","owner":"p1","rcl":6},
        {"name":"W2N1","owner":"p1","rcl":6}]}))?;
    for (room, x) in [("W1N1", 10), ("W2N1", 20)] {
        e.place_object(
            room,
            "structure",
            &json!({"pos":[x,10],"structureType":"source"}),
        )?;
    }
    Ok(e)
}

#[test]
fn room_prototype_wrappers_persist_and_borrowed_queries_use_receiver() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let code = r#"
            const a=Game.rooms.W1N1,b=Game.rooms.W2N1;
            globalThis.nativeRoomFind=Room.prototype.find;
            globalThis.wrapCalls=0;
            Room.prototype.find=function(...args){wrapCalls++;return nativeRoomFind.apply(this,args)};
            const names=['find','lookAt','lookForAt','lookAtArea','lookForAtArea','findPath',
                'createConstructionSite','createFlag','getEventLog','getTerrain','getPositionAt','findExitTo','toJSON'];
            a.memory={mark:'a'}; b.memory={mark:'b'};
            [Object.getOwnPropertyNames(a).filter(k=>typeof a[k]==='function'),
                names.every(k=>typeof Room.prototype[k]==='function'&&a[k]===Room.prototype[k]),
                a instanceof Room,a.find(FIND_SOURCES)[0].pos.x,
                a.find.call(b,FIND_SOURCES)[0].pos.x,wrapCalls,a.memory.mark,b.memory.mark,
                a.getPositionAt.call(b,4,5).roomName,a.lookForAt.call(b,LOOK_SOURCES,20,10).length]
        "#;
        assert_eq!(
            e.run_player("p1", code)?.expr["value"],
            json!([[], true, true, 10, 20, 2, "a", "b", "W2N1", 1])
        );
        // 下一次投影重建实例，但不得重装原型或丢失玩家包装。
        let code = r#"
            const a=Game.rooms.W1N1,b=Game.rooms.W2N1;
            [a.find===Room.prototype.find,a.find!==nativeRoomFind,
                a.find(FIND_SOURCES)[0].pos.x,b.find(FIND_SOURCES)[0].pos.x,
                wrapCalls,a.memory.mark,b.memory.mark,
                Object.getOwnPropertyNames(a).filter(k=>typeof a[k]==='function')]
        "#;
        assert_eq!(
            e.run_player("p1", code)?.expr["value"],
            json!([true, true, 10, 20, 4, "a", "b", []])
        );
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        assert_eq!(restored.run_player("p1", "[typeof globalThis.wrapCalls,Game.rooms.W1N1.find(FIND_SOURCES)[0].pos.x,Game.rooms.W2N1.find(FIND_SOURCES)[0].pos.x,Game.rooms.W1N1.memory.mark]")?.expr["value"],json!(["undefined",10,20,"a"]));
    }
    Ok(())
}

#[test]
fn room_prototype_creation_and_event_buffers_remain_room_local() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let code = r#"
            const a=Game.rooms.W1N1,b=Game.rooms.W2N1;
            const log=a.getEventLog();log.push({event:999});
            [a.createConstructionSite(15,15,STRUCTURE_ROAD),
                a.createConstructionSite.call(b,16,16,STRUCTURE_ROAD),
                a.createFlag.call(b,21,21,'OtherRoom'),Game.flags.OtherRoom.pos.roomName,
                a.getEventLog()===log,a.getEventLog(true),b.getEventLog().length,
                a.lookAt.call(b,20,10).some(o=>o.type===LOOK_SOURCES),
                b.lookForAtArea(LOOK_SOURCES,9,19,11,21,true).length,
                b.lookAtArea(10,20,10,20,true).some(o=>o.type===LOOK_SOURCES),
                a.findPath(new RoomPosition(5,5,'W1N1'),new RoomPosition(7,5,'W1N1')).length>0,
                JSON.parse(JSON.stringify(a)).name]
        "#;
        assert_eq!(
            e.run_player("p1", code)?.expr["value"],
            json!([
                0,
                0,
                "OtherRoom",
                "W2N1",
                true,
                "[]",
                0,
                true,
                1,
                true,
                true,
                "W1N1"
            ])
        );
        // 指令经过真实结算，各房创建消费者必须读取自己的私有对象列表。
        for room in ["W1N1", "W2N1"] {
            assert_eq!(
                e.find_in_room(room, kk_core::constants::FIND_CONSTRUCTION_SITES)
                    .len(),
                1
            );
        }
        assert_eq!(e.run_player("p1","[Game.rooms.W1N1.getEventLog().some(o=>o.event===999),Game.flags.OtherRoom.pos.roomName]")?.expr["value"],json!([false,"W2N1"]));
    }
    Ok(())
}

/// 官方 flags.js:28 继承 RoomObject；Overmind 以 ref 补丁生成持久 overlord 引用。
#[test]
fn flag_room_object_patch_survives_projection_and_cold_restore() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let patch = r#"Object.defineProperty(RoomObject.prototype,'ref',{
            get(){return this.id || this.name || ''},configurable:true});"#;
        let create = format!(
            r#"{patch}
            Game.rooms.W1N1.createFlag(21,21,'Mine');
            const flag=Game.flags.Mine;
            flag.memory.overlord=flag.ref+'>mine';
            [flag instanceof Flag,flag instanceof RoomObject,flag.ref,flag.memory.overlord]
        "#
        );
        assert_eq!(
            e.run_player("p1", &create)?.expr["value"],
            json!([true, true, "Mine", "Mine>mine"])
        );
        assert_eq!(
            e.run_player(
                "p1",
                r#"[Game.flags.Mine instanceof RoomObject,
            Game.flags.Mine.ref,Game.flags.Mine.memory.overlord]"#
            )?
            .expr["value"],
            json!([true, "Mine", "Mine>mine"])
        );
        let saved = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        // VM 补丁需在新进程重新装载；持久引用随玩家 Memory 保存。
        let read = format!(
            r#"{patch}[Game.flags.Mine instanceof RoomObject,
            Game.flags.Mine.ref,Game.flags.Mine.memory.overlord]"#
        );
        assert_eq!(
            restored.run_player("p1", &read)?.expr["value"],
            json!([true, "Mine", "Mine>mine"])
        );
    }
    Ok(())
}
