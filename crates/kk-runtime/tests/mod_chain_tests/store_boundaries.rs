//! Mod 库存对象的业务字段不能触发内建结构的专属资源门。
use super::*;

#[test]
fn terminal_named_mod_fields_do_not_block_creep_withdraw() -> anyhow::Result<()> {
    let source = package(
        r#"
api.objects.define('cargo:crate', {version:1,playerClass:'Crate',positionFields:['x','y'],capacity:100,fields:{
  x:{type:'u32',min:0,max:49},y:{type:'u32',min:0,max:49},structureType:{type:'string',maxBytes:32},
  effects:{type:'array',maxLen:4,element:{type:'record',fields:{power:{type:'u32'},ticksRemaining:{type:'u32'}}}}
}});
"#,
    );
    let player = "export default function install(api){class Crate extends api.builtins.RoomObject{get effects(){return api.view.field(this,'effects');}} api.objects.bind('cargo:crate',Crate);}";
    for binary in [false, true] {
        let mut e =
            engine_with_player_files(&[("cargo", source.clone())], &[("cargo", player)], binary)?;
        let actor = harness::fighter(&mut e, "a", 10, 10, &["carry", "move"])?;
        let target = e.place_mod_object("W1N1","cargo:crate", &json!({"x":10,"y":11,"structureType":"terminal","effects":[{"power":15,"ticksRemaining":10}]}))?;
        let fields = e.run_player(
            "p1",
            &format!("Object.keys(Game.getObjectById('{target}').effects[0]).sort()"),
        )?;
        assert_eq!(fields.expr["value"], json!(["power", "ticksRemaining"]));
        let transfer = e.run_player(
            "p1",
            &format!("Game.creeps.a.transfer(Game.getObjectById('{target}'),'energy',10)"),
        )?;
        assert_eq!(transfer.expr["value"], 0);
        let withdraw = e.run_player(
            "p1",
            &format!("Game.creeps.a.withdraw(Game.getObjectById('{target}'),'energy',10)"),
        )?;
        assert_eq!(
            withdraw.expr["value"], 0,
            "Mod 业务字段不应被误认成终端封锁：{binary}"
        );
        assert_eq!(
            e.get_object(&target).unwrap()["fields"]["store"]["energy"],
            0
        );
        assert_eq!(e.get_object(&actor).unwrap()["store"]["energy"], 30);
    }
    Ok(())
}
