//! 冻结诊断与真实链执行、故障恢复和停用闭包保持一致。
use super::*;
use std::collections::BTreeSet;

fn report(e: &EmbedEngine) -> Value {
    serde_json::to_value(e.mod_composition().unwrap()).unwrap()
}
fn slot<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap()
}

#[test]
fn forward_cross_package_dependencies_match_actual_chain_and_disable_transitively()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let own = package(
            "api.objects.define('chain:node',{version:1,playerClass:'Node',visibility:'server-only',fields:{trace:{type:'string',maxBytes:1024,default:''}},tick(ctx){ctx.target.fields.trace+='C';ctx.state={n:(ctx.state?.n||0)+1};}});",
        );
        let hooks = |id: &str| {
            package(&format!(
                r#"
for(const kind of ['before','wrap','after']){{
 const dependsOn='{id}'==='a'?['z:handler.'+kind]:[];
 api.handlers[kind]('chain:node.tick',{{id:kind,dependsOn}},(ctx,next)=>{{
  ctx.target.fields.trace+='{id}'+kind[0];
  if(kind==='wrap'){{next();ctx.target.fields.trace+='{id}W';}}
 }});
}}
"#
            ))
        };
        let healthy = package(
            "api.handlers.before('chain:node.tick',{id:'healthy'},ctx=>{ctx.target.fields.trace+='H'});",
        );
        let mut e = engine_with_dependencies(
            &[
                ("chain", own),
                ("a", hooks("a")),
                ("z", hooks("z")),
                ("m", package("")),
                ("h", healthy),
            ],
            binary,
            &[("m", "a")],
        )?;
        let id = e.place_mod_object("W1N1", "chain:node", &json!({}))?;
        let initial = report(&e);
        let active = &slot(&initial, "chain:node.tick")["active"];
        assert_eq!(
            active["before"],
            json!(["h:handler.healthy", "z:handler.before", "a:handler.before"])
        );
        assert_eq!(active["wraps"], json!(["z:handler.wrap", "a:handler.wrap"]));
        assert_eq!(
            active["after"],
            json!(["a:handler.after", "z:handler.after"])
        );
        assert_eq!(initial["disable_closures"]["z"], json!(["a", "m", "z"]));
        e.tick(1)?;
        assert_eq!(
            e.get_object(&id).unwrap()["fields"]["trace"],
            "HzbabzwawCaWzWaaza"
        );
        assert_eq!(
            e.disable_mod("z")?,
            BTreeSet::from(["a".into(), "m".into(), "z".into()])
        );
        let filtered = report(&e);
        assert_eq!(filtered["disabled_mods"], initial["disable_closures"]["z"]);
        assert_eq!(filtered["plan_hash"], initial["plan_hash"]);
        assert_eq!(filtered["rules"], initial["rules"]);
        assert_eq!(
            slot(&filtered, "chain:node.tick")["active"]["wraps"],
            json!([])
        );
        e.tick(1)?;
        assert_eq!(
            e.get_object(&id).unwrap()["fields"]["trace"],
            "HzbabzwawCaWzWaazaHC"
        );
        assert_eq!(report(&e), filtered, "重建不得改变诊断");
        e.disable_mod("chain")?;
        let disabled = report(&e);
        assert!(slot(&disabled, "chain:node.tick")["active"].is_null());
        assert_eq!(slot(&disabled, "chain:node.tick")["execution"], "inactive");
        let before = e.get_object(&id);
        e.tick(1)?;
        assert_eq!(e.get_object(&id), before);
    }
    Ok(())
}

#[test]
fn fault_disable_report_restores_original_center_before_cached_retry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let bad = package(
            "api.handlers.replace('builtin:link.transfer',{id:'replace'},()=>{throw Error('bad')});",
        );
        let dependent = package(
            "api.handlers.before('builtin:link.tick',{id:'dependent'},()=>{throw Error('dependent')});",
        );
        let mut e = engine_with_dependencies(
            &[("bad", bad), ("dependent", dependent)],
            binary,
            &[("dependent", "bad")],
        )?;
        let source = link(&mut e, 20, 100, 0)?;
        let target = link(&mut e, 21, 0, 0)?;
        let initial = report(&e);
        assert_eq!(
            slot(&initial, "builtin:link.transfer")["original_center"],
            "builtin:link.transfer.native"
        );
        assert_eq!(
            slot(&initial, "builtin:link.transfer")["active"]["center"],
            "bad:handler.replace"
        );
        assert!(e.run_player("p1",&format!("globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25);'cached'")).is_err());
        assert_eq!(report(&e), initial);
        e.disable_mod("bad")?;
        let fallback = report(&e);
        assert_eq!(fallback["disabled_mods"], json!(["bad", "dependent"]));
        let active = slot(&fallback, "builtin:link.transfer");
        assert_eq!(active["execution"], "hosted");
        assert_eq!(active["active"]["center"], active["original_center"]);
        assert_eq!(active["active"]["native_center"], true);
        assert_eq!(
            e.run_player("p1", "throw Error('must not rerun')")?.expr["value"],
            "cached"
        );
        assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], 24);
        assert_eq!(report(&e), fallback);
        assert_eq!(
            e.run_player("p1", "Memory.n===1&&globalThis.n===1")?.expr["value"],
            true
        );
    }
    assert!(EmbedEngine::new().mod_composition().is_none());
    Ok(())
}
