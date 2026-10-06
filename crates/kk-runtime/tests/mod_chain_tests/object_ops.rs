//! 同房对象能力、出生/删除资格、正常拒绝与整轮恢复。
use super::*;

fn source(tick: &str, action: &str) -> String {
    package(&format!(
        r#"
const node=api.objects.define('chain:node',{{version:1,playerClass:'Node',visibility:'server-only',ownerField:'owner',positionFields:['x','y'],fields:{{
owner:{{type:'string',maxBytes:128,default:'p1'}},x:{{type:'u32',min:0,max:49}},y:{{type:'u32',min:0,max:49}},value:{{type:'i32',default:0}}
}},tick(ctx){{{tick}}}}});
for(const name of ['a-accept','b-decline','c-accept'])api.intents.define('chain:'+name,{{scope:'object',target:node,args:{{type:'record',fields:{{id:{{type:'string',maxBytes:128}}}}}},process(ctx){{{action}}}}});
"#
    ))
}

fn node(e: &mut EmbedEngine, room: &str, x: u8) -> anyhow::Result<String> {
    e.place_mod_object(room, "chain:node", &json!({"x":x,"y":12}))
}

#[test]
fn tick_creates_children_next_round_and_self_deletion_invalidates_id() -> anyhow::Result<()> {
    for binary in [false, true] {
        let body = r#"
if(ctx.target.fields.x===10){
  if(ctx.tick===0){const child=ctx.objects.create('chain:node',{x:20,y:12});ctx.state={child:child.id,created:child.fields.value};}
  ctx.target.fields.value++;
}else{
  ctx.target.fields.value++;
  if(ctx.target.fields.value===2){
    const id=ctx.target.id;ctx.objects.remove(id);
    if(ctx.target.fields!==null||ctx.objects.get(id)!==null)throw Error('deleted reference alive');
    ctx.state.deleted=true;
  }
}
"#;
        let mut e = engine(&[("chain", source(body, ""))], binary)?;
        let root = node(&mut e, "W1N1", 10)?;
        e.tick(1)?;
        let state = e.engine_mod_state("chain");
        let child = state["child"].as_str().unwrap();
        assert_eq!(state["created"], 0);
        assert_eq!(e.get_object(child).unwrap()["fields"]["value"], 0);
        e.tick(1)?;
        assert_eq!(e.get_object(child).unwrap()["fields"]["value"], 1);
        e.tick(1)?;
        assert!(e.get_object(child).is_none());
        assert_eq!(e.get_object(&root).unwrap()["fields"]["value"], 3);
        assert_eq!(e.engine_mod_state("chain")["deleted"], true);
    }
    Ok(())
}

#[test]
fn actions_create_before_tick_and_deleted_later_object_is_not_dispatched() -> anyhow::Result<()> {
    for binary in [false, true] {
        let body = "ctx.target.fields.value++;ctx.state={order:[...(ctx.state?.order||[]),ctx.target.fields.x]};if(ctx.target.fields.x===10){const peers=ctx.objects.list('chain:node').objects;for(const peer of peers)if(peer.fields.x===20)ctx.objects.remove(peer.id);}";
        let action = "ctx.objects.create('chain:node',{x:30,y:12});";
        let mut e = engine(&[("chain", source(body, action))], binary)?;
        let root = node(&mut e, "W1N1", 10)?;
        let victim = node(&mut e, "W1N1", 20)?;
        e.run_player(
            "p1",
            &format!("__kkEmitExtensionIntent('chain:a-accept','{root}',{{id:'{victim}'}})"),
        )?;
        assert!(e.get_object(&victim).is_none());
        assert_eq!(e.engine_mod_state("chain"), json!({"order":[10,30]}));
    }
    Ok(())
}

#[test]
fn foreign_types_native_objects_other_rooms_and_native_slots_do_not_gain_write_access()
-> anyhow::Result<()> {
    for write in [
        "ctx.objects.update(id,{value:1});",
        "ctx.objects.remove(id);",
    ] {
        let foreign = package(
            "api.objects.define('foreign:node',{version:1,playerClass:'ForeignNode',visibility:'server-only',fields:{value:{type:'i32',default:0}}});",
        );
        let mut e = engine(
            &[
                (
                    "chain",
                    source(
                        &format!("const id='kk0000000000000003';try{{{write}}}catch(e){{}}"),
                        "",
                    ),
                ),
                ("foreign", foreign),
            ],
            false,
        )?;
        node(&mut e, "W1N1", 10)?;
        let id = e.place_mod_object("W1N1", "foreign:node", &json!({}))?;
        assert_eq!(id, kk_world::ObjId(3).as_str());
        assert!(e.tick(1).is_err());
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 0);
    }
    for body in [
        "try{ctx.objects.get('kk0000000000000001')}catch(e){}",
        "try{ctx.objects.remove('kk0000000000000001')}catch(e){}",
    ] {
        let mut e = engine(&[("chain", source(body, ""))], false)?;
        node(&mut e, "W1N1", 10)?;
        assert!(e.tick(1).is_err());
        assert_eq!(e.get_game_time(), 0);
    }
    for operation in ["update", "remove"] {
        let body = format!(
            "if(ctx.room==='W1N1'){{if(ctx.objects.get('kk0000000000000004')!==null)throw Error('cross room read');try{{ctx.objects.{operation}('kk0000000000000004',{{value:1}})}}catch(e){{}}}}"
        );
        let mut e = engine_with_shard(
            &[("chain", source(&body, ""))],
            true,
            &[],
            RegisterOptions::default(),
            &json!({
                "players":["p1"],"rooms":[{"name":"W1N1","owner":"p1"},{"name":"W2N2","owner":"p1"}]
            }),
        )?;
        node(&mut e, "W1N1", 10)?;
        let remote = node(&mut e, "W2N2", 20)?;
        assert_eq!(remote, kk_world::ObjId(4).as_str());
        assert!(e.tick(1).is_err());
        assert_eq!(e.get_object(&remote).unwrap()["fields"]["value"], 0);
    }
    let foreign_slot = package(
        "api.handlers.before('builtin:link.tick',{id:'no-objects'},ctx=>{try{ctx.objects.create('chain:node',{x:20,y:12})}catch(e){}});",
    );
    let mut e = engine(&[("chain", source("", "")), ("other", foreign_slot)], false)?;
    let link_id = link(&mut e, 20, 0, 10)?;
    assert!(e.tick(1).is_err());
    assert_eq!(cooldown(&e, &link_id), 10);
    Ok(())
}

#[test]
fn ended_object_capability_cannot_be_used_in_the_next_tick() -> anyhow::Result<()> {
    let source = source(
        "if(saved){try{saved('chain:node',{x:30,y:12})}catch(e){}}saved=ctx.objects.create;ctx.state={count:ctx.objects.list().objects.length};",
        "",
    );
    let source = format!("let saved;{source}");
    let mut e = engine(&[("chain", source)], false)?;
    node(&mut e, "W1N1", 10)?;
    e.tick(3)?;
    assert_eq!(e.engine_mod_state("chain"), json!({"count":1}));
    Ok(())
}

#[test]
fn declined_action_restores_peer_movement_deletion_creation_and_id_cursor() -> anyhow::Result<()> {
    for binary in [false, true] {
        let action = r#"
if(name==='b-decline'){
  ctx.objects.update(ctx.args.id,{x:25,value:999});ctx.objects.remove(ctx.args.id);
  ctx.objects.remove(ctx.target.id);
  const fresh=ctx.objects.create('chain:node',{x:30,y:12});ctx.objects.update(fresh.id,{x:31});ctx.objects.remove(fresh.id);
  ctx.state={dirty:true};return api.decline('no');
}
const made=ctx.objects.create('chain:node',{x:name==='a-accept'?40:41,y:12});
ctx.state={ids:[...(ctx.state?.ids||[]),made.id]};
"#;
        let mut e = engine(&[("chain", source("", action))], binary)?;
        let root = node(&mut e, "W1N1", 10)?;
        let peer = node(&mut e, "W1N1", 20)?;
        let before_root = e.get_object(&root).unwrap();
        let before_peer = e.get_object(&peer).unwrap();
        e.run_player("p1", &format!("for(const name of ['a-accept','b-decline','c-accept'])__kkEmitExtensionIntent('chain:'+name,'{root}',{{id:'{peer}'}});Memory.accepted=true;"))?;
        assert_eq!(e.get_object(&root).unwrap(), before_root);
        assert_eq!(e.get_object(&peer).unwrap(), before_peer);
        let state = e.engine_mod_state("chain");
        let ids = state["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 2);
        let a = kk_world::ObjId::parse(ids[0].as_str().unwrap()).unwrap();
        let c = kk_world::ObjId::parse(ids[1].as_str().unwrap()).unwrap();
        assert_eq!(c.0, a.0 + 1, "拒绝期间分配的 ID 不能消耗游标");
        assert_eq!(
            e.get_object(ids[0].as_str().unwrap()).unwrap()["fields"]["x"],
            40
        );
        assert_eq!(
            e.get_object(ids[1].as_str().unwrap()).unwrap()["fields"]["x"],
            41
        );
        assert!(state.get("dirty").is_none());
        assert!(e.last_mod_settlement_fault().is_none());
    }
    Ok(())
}

#[test]
fn pagination_and_updates_observe_current_fields_but_return_detached_snapshots()
-> anyhow::Result<()> {
    let body = r#"
if(ctx.target.fields.x!==10)return;
const fields=ctx.target.fields;fields.value=7;
if(ctx.objects.get(ctx.target.id).fields.value!==7)throw Error('pending value missing');
ctx.objects.update(ctx.target.id,{value:8});
if(fields.value!==8)throw Error('cached fields not refreshed');
let after='',ids=[];
do{const page=ctx.objects.list('chain:node',{after,limit:2});ids.push(...page.objects.map(o=>o.id));after=page.next;}while(after);
const detached=ctx.objects.get(ctx.target.id);detached.fields.value=123;
if(ctx.objects.get(ctx.target.id).fields.value!==8)throw Error('snapshot wrote candidate');
ctx.state={ids};
"#;
    let mut e = engine(&[("chain", source(body, ""))], true)?;
    let mut ids = vec![node(&mut e, "W1N1", 10)?];
    for x in 20..27 {
        ids.push(node(&mut e, "W1N1", x)?);
    }
    e.tick(1)?;
    assert_eq!(e.engine_mod_state("chain"), json!({"ids":ids}));
    assert_eq!(e.get_object(&ids[0]).unwrap()["fields"]["value"], 8);
    Ok(())
}

#[test]
fn caught_faults_after_creating_or_deleting_cannot_publish_candidates() -> anyhow::Result<()> {
    for fault in [
        "ctx.objects.update(ctx.target.id,{unknown:1});",
        "ctx.objects.create('foreign:node',{x:20,y:12});",
        "ctx.objects.update('kk0000000000999999',{value:1});",
        "ctx.objects.remove('kk0000000000999999');",
        "ctx.objects.create('chain:node',{x:50,y:12});",
        "ctx.objects.list('chain:node',{limit:1.5});",
        "ctx.objects.get('invalid');",
        "ctx.objects.create('chain:node',{get x(){throw Error('getter');},y:12});",
        "ctx.objects.create('chain:node',{get x(){ctx.objects.create('chain:node',{x:40,y:12});throw Error('getter');},y:12});",
        "for(let n=0;n<300;n++)ctx.objects.create('chain:node',{x:20,y:12});",
    ] {
        let body = format!(
            "const made=ctx.objects.create('chain:node',{{x:30,y:12}});ctx.objects.remove(made.id);ctx.target.fields.value++;try{{{fault}}}catch(e){{}}ctx.state={{dirty:true}};"
        );
        let mut e = engine(&[("chain", source(&body, ""))], false)?;
        let root = node(&mut e, "W1N1", 10)?;
        let before = (e.world_snapshot(), e.mod_object_snapshots());
        assert!(e.tick(1).is_err(), "{fault}");
        assert_eq!(
            (e.world_snapshot(), e.mod_object_snapshots()),
            before,
            "{fault}"
        );
        assert_eq!(e.get_object(&root).unwrap()["fields"]["value"], 0);
        assert_eq!(e.engine_mod_state("chain"), Value::Null);
    }
    Ok(())
}

#[test]
fn late_writeback_fault_retries_cached_players_with_same_creation_ids() -> anyhow::Result<()> {
    for binary in [false, true] {
        let body = "if(ctx.target.fields.x===10){const child=ctx.objects.create('chain:node',{x:30,y:12});ctx.objects.update(ctx.target.id,{x:11});ctx.state={id:child.id};}";
        let mut e = engine(&[("chain", source(body, ""))], binary)?;
        let root = node(&mut e, "W1N1", 10)?;
        let before = (e.world_snapshot(), e.mod_object_snapshots());
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::Segments);
        assert!(
            e.run_player(
                "p1",
                "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;'cached'"
            )
            .is_err()
        );
        assert_eq!((e.world_snapshot(), e.mod_object_snapshots()), before);
        assert_eq!(e.engine_mod_state("chain"), Value::Null);
        let result = e.run_player("p1", "throw Error('must not rerun')")?;
        assert_eq!(result.expr["value"], "cached");
        let child = e.engine_mod_state("chain")["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            kk_world::ObjId::parse(&child).unwrap().0,
            kk_world::ObjId::parse(&root).unwrap().0 + 1
        );
        assert_eq!(e.get_object(&root).unwrap()["fields"]["x"], 11);
        assert_eq!(
            e.run_player("p1", "Memory.n===1&&globalThis.n===1")?.expr["value"],
            true
        );
    }
    Ok(())
}

#[test]
fn two_room_mutations_roll_back_then_retry_and_owner_disable_preserves_objects()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let own = source(
            "if(ctx.target.fields.x<30){const child=ctx.objects.create('chain:node',{x:30,y:12});ctx.objects.remove(ctx.target.id);ctx.state={ids:[...(ctx.state?.ids||[]),child.id]};}",
            "",
        );
        let bad = package(
            "api.handlers.after('chain:node.tick',{id:'fault'},ctx=>{if(ctx.room==='W2N2')throw Error('second room');});",
        );
        let mut e = engine_with_shard(
            &[("chain", own), ("bad", bad)],
            binary,
            &[],
            RegisterOptions::default(),
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1"},{"name":"W2N2","owner":"p1"}]}),
        )?;
        let first = node(&mut e, "W1N1", 10)?;
        let second = node(&mut e, "W2N2", 20)?;
        let before = (e.world_snapshot(), e.mod_object_snapshots());
        assert!(
            e.run_player(
                "p1",
                "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;'cached'"
            )
            .is_err()
        );
        assert_eq!((e.world_snapshot(), e.mod_object_snapshots()), before);
        assert_eq!(e.engine_mod_state("chain"), Value::Null);
        assert_eq!(e.last_mod_settlement_fault().unwrap().room, "W2N2");
        e.disable_mod("bad")?;
        assert_eq!(
            e.run_player("p1", "throw Error('cannot rerun')")?.expr["value"],
            "cached"
        );
        assert!(e.get_object(&first).is_none());
        assert!(e.get_object(&second).is_none());
        let state = e.engine_mod_state("chain");
        let ids = state["ids"].as_array().unwrap();
        let next = kk_world::ObjId::parse(&second).unwrap().0 + 1;
        for (offset, id) in ids.iter().enumerate() {
            assert_eq!(
                kk_world::ObjId::parse(id.as_str().unwrap()).unwrap().0,
                next + offset as u64
            );
        }
        assert_eq!(ids.len(), 2);
        assert_eq!(
            e.run_player("p1", "Memory.n===1&&globalThis.n===1")?.expr["value"],
            true
        );
        // 停用后连本应自主删除/生成的对象也保留，冻结 schema 与 state 不被卸载。
        node(&mut e, "W1N1", 10)?;
        let objects = e.mod_object_snapshots();
        e.disable_mod("chain")?;
        e.tick(3)?;
        assert_eq!(e.mod_object_snapshots(), objects);
        assert_eq!(e.engine_mod_state("chain"), state);
    }
    Ok(())
}

#[test]
fn caught_id_exhaustion_and_decline_never_publish_or_write_player_candidates() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for ending in ["return api.decline('no');", "return;"] {
            let action = format!(
                "try{{ctx.objects.create('chain:node',{{x:30,y:12}});ctx.objects.create('chain:node',{{x:31,y:12}});}}catch(e){{}}{ending}"
            );
            let mut e = engine(&[("chain", source("", &action))], binary)?;
            let root = node(&mut e, "W1N1", 10)?;
            e.bump_next_obj_id_for_test(u64::MAX - 1)?;
            let before = (e.world_snapshot(), e.mod_object_snapshots());
            let player = e.player_state_for_test("p1").unwrap().clone();
            let code = format!(
                "Memory.dirty=true;__kkEmitExtensionIntent('chain:b-decline','{root}',{{id:'{root}'}});'cached'"
            );
            assert!(e.run_player("p1", &code).is_err());
            assert_eq!((e.world_snapshot(), e.mod_object_snapshots()), before);
            assert_eq!(
                serde_json::to_value(e.player_state_for_test("p1"))?,
                serde_json::to_value(Some(&player))?
            );
            assert!(e.run_player("p1", "throw Error('cannot rerun')").is_err());
            assert_eq!((e.world_snapshot(), e.mod_object_snapshots()), before);
        }
    }
    Ok(())
}
