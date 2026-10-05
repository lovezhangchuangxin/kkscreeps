//! 真实 JS/native 整槽链：链序、候选同步、取消、失败回滚与能力寿命。
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_mod_host::{RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct PackageRoot(PathBuf);
impl Drop for PackageRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const COUNTER: &str = r#"
const counter = api.objects.define('chain:counter', {
  version:1, playerClass:'Counter', visibility:'server-only', ownerField:'owner', positionFields:['x','y'],
  fields:{owner:{type:'string',maxBytes:128},x:{type:'u32',min:0,max:49},y:{type:'u32',min:0,max:49},value:{type:'i32',default:0}}
});
api.intents.define('chain:increment', {
  scope:'object', target:counter, args:{type:'record',fields:{amount:{type:'i32',min:1,max:10}}},
  process(ctx) { ctx.target.fields.value += ctx.args.amount; ctx.state.trace.push('process'); }
});
"#;

fn engine(packages: &[(&str, String)], kkpv: bool) -> anyhow::Result<EmbedEngine> {
    engine_with_dependencies(packages, kkpv, &[])
}

fn engine_with_dependencies(
    packages: &[(&str, String)],
    kkpv: bool,
    dependencies: &[(&str, &str)],
) -> anyhow::Result<EmbedEngine> {
    engine_with_options(packages, kkpv, dependencies, RegisterOptions::default())
}

fn engine_with_options(
    packages: &[(&str, String)],
    kkpv: bool,
    dependencies: &[(&str, &str)],
    options: RegisterOptions,
) -> anyhow::Result<EmbedEngine> {
    engine_with_shard(
        packages,
        kkpv,
        dependencies,
        options,
        &json!({
            "players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]
        }),
    )
}

fn engine_with_shard(
    packages: &[(&str, String)],
    kkpv: bool,
    dependencies: &[(&str, &str)],
    options: RegisterOptions,
    shard: &Value,
) -> anyhow::Result<EmbedEngine> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-chain-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    for (id, source) in packages {
        let dir = root.0.join(id);
        std::fs::create_dir_all(&dir)?;
        let dependencies: serde_json::Map<String, Value> = dependencies
            .iter()
            .filter(|(dependent, _)| dependent == id)
            .map(|(_, dependency)| (dependency.to_string(), json!("^1.0.0")))
            .collect();
        std::fs::write(
            dir.join("mod.json"),
            json!({"id": id, "version": "1.0.0", "apiVersion": "^1.0.0", "dependencies": dependencies}).to_string(),
        )?;
        std::fs::write(dir.join("server.js"), source)?;
    }
    let packages = discover_packages(&root.0)?;
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(kkpv);
    e.create_shard_with_mods(shard, &packages, options)?;
    Ok(e)
}

fn package(body: &str) -> String {
    format!("export default function register(api) {{ {body} }}")
}

fn link(e: &mut EmbedEngine, x: u8, energy: u32, cooldown: u32) -> anyhow::Result<String> {
    e.place_object("W1N1", "structure", &json!({"pos": [x, 20], "structureType": "link", "owner": "p1", "store": {"energy": energy}, "cooldown": cooldown}))
}

fn cooldown(e: &EmbedEngine, id: &str) -> Value {
    e.get_object(id).unwrap()["cooldown"].clone()
}

fn counter(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_mod_object(
        "W1N1",
        "chain:counter",
        &json!({"owner":"p1","x":10,"y":12}),
    )
}
fn increment(e: &mut EmbedEngine, id: &str) -> anyhow::Result<()> {
    e.run_player(
        "p1",
        &format!(
            "__kkEmitExtensionIntent('chain:increment','{id}',{{amount:3}});Memory.chainAttempt=1"
        ),
    )?;
    Ok(())
}

// 三个同对象动作依稳定键序执行；中间动作拒绝不能丢失前一个动作的结果。
fn decline_package(rejection: &str) -> String {
    format!(
        r#"
let calls = 0;
export default function register(api) {{
  const counter = api.objects.define('a:counter', {{version:1,playerClass:'Counter',visibility:'server-only',ownerField:'owner',positionFields:['x','y'],fields:{{
    owner:{{type:'string',maxBytes:128}},x:{{type:'u32',min:0,max:49}},y:{{type:'u32',min:0,max:49}},value:{{type:'i32',default:0}}
  }}}});
  for (const name of ['a-accept','b-test','c-accept']) {{
    api.intents.define('a:'+name,{{scope:'object',target:counter,args:{{type:'record',fields:{{}}}},process(ctx){{
      calls++;
      ctx.target.fields.value++;
      ctx.state={{count:(ctx.state?.count||0)+1,calls}};
      if(name==='b-test') {{ {rejection} }}
    }}}});
  }}
  api.handlers.before('a:b-test.slot',{{id:'pre'}},ctx=>{{ctx.target.fields.x=21;ctx.state={{count:100}};}});
  api.handlers.wrap('a:b-test.slot',{{id:'outer'}},(ctx,next)=>{{
    const fields=ctx.target.fields;
    try {{ next(); }} catch(e) {{}}
    if (fields.value!==7) throw Error('next 后字段未同步');
    ctx.state={{count:999}};
  }});
  api.handlers.after('a:b-test.slot',{{id:'after'}},ctx=>{{ctx.state={{count:888}};}});
}}
"#
    )
}

// 共享夹具与契约说明见 mod_chain_tests/README.md。
#[path = "mod_chain_tests/harness.rs"]
mod harness;

#[path = "mod_chain_tests/synchronization.rs"]
mod synchronization;

#[path = "mod_chain_tests/faults.rs"]
mod faults;

#[path = "mod_chain_tests/decline.rs"]
mod decline;

#[path = "mod_chain_tests/disable.rs"]
mod disable;

#[path = "mod_chain_tests/auto_disable.rs"]
mod auto_disable;
#[path = "mod_chain_tests/creep_build.rs"]
mod creep_build;
#[path = "mod_chain_tests/creep_build_world.rs"]
mod creep_build_world;
#[path = "mod_chain_tests/creep_combat.rs"]
mod creep_combat;
#[path = "mod_chain_tests/creep_combat_world.rs"]
mod creep_combat_world;
#[path = "mod_chain_tests/creep_move.rs"]
mod creep_move;
#[path = "mod_chain_tests/creep_move_world.rs"]
mod creep_move_world;
#[path = "mod_chain_tests/creep_store.rs"]
mod creep_store;
#[path = "mod_chain_tests/damage_policy.rs"]
mod damage_policy;
#[path = "mod_chain_tests/heal_policy.rs"]
mod heal_policy;
#[path = "mod_chain_tests/link_decline.rs"]
mod link_decline;
#[path = "mod_chain_tests/mass_policy.rs"]
mod mass_policy;
#[path = "mod_chain_tests/mass_policy_faults.rs"]
mod mass_policy_faults;
#[path = "mod_chain_tests/mod_building_world.rs"]
mod mod_building_world;
#[path = "mod_chain_tests/object_ops.rs"]
mod object_ops;
#[path = "mod_chain_tests/object_tick.rs"]
mod object_tick;
#[path = "mod_chain_tests/resource_domain.rs"]
mod resource_domain;
#[path = "mod_chain_tests/resource_faults.rs"]
mod resource_faults;
#[path = "mod_chain_tests/store_rules.rs"]
mod store_rules;

#[path = "mod_chain_tests/composition.rs"]
mod composition;

#[path = "mod_chain_tests/incoming_faults.rs"]
mod incoming_faults;
#[path = "mod_chain_tests/incoming_policy.rs"]
mod incoming_policy;
#[path = "mod_chain_tests/move_admission.rs"]
mod move_admission;
#[path = "mod_chain_tests/move_contest.rs"]
mod move_contest;
#[path = "mod_chain_tests/move_obstacle.rs"]
mod move_obstacle;
#[path = "mod_chain_tests/retaliation_faults.rs"]
mod retaliation_faults;
#[path = "mod_chain_tests/retaliation_policy.rs"]
mod retaliation_policy;
#[path = "mod_chain_tests/tough_damage.rs"]
mod tough_damage;
