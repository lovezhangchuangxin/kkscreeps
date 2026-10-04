//! 双玩家、双房间的 Mod 玩家 API、统一发布与缓存恢复组合验收。
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_engine::{PlayerState, WritebackStep};
use kk_mod_host::{RegisterOptions, discover_packages};
use kk_runtime::{EmbedEngine, PlayerRun};
use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
const PLAYERS: [&str; 2] = ["p1", "p2"];
const ROOMS: [&str; 2] = ["W1N1", "W2N1"];

struct PackageRoot(PathBuf);
impl Drop for PackageRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SERVER_JS: &str = r#"
let calls = 0;
export default function register(api) {
  const counter = api.objects.define('gameplay:counter', {
    version:1, playerClass:'Counter', visibility:'room-visible',
    ownerField:'owner', positionFields:['x','y'], fields:{
      owner:{type:'string',maxBytes:128},
      x:{type:'u32',min:0,max:49}, y:{type:'u32',min:0,max:49},
      value:{type:'i32',default:0,visibility:'public'},
      secret:{type:'i32',default:42,visibility:'owner'},
      audit:{type:'i32',default:99,visibility:'server-only'},
    },
  });
  api.intents.define('gameplay:increment', {
    scope:'object', target:counter,
    args:{type:'record',fields:{amount:{type:'i32',min:1,max:10}}},
    process(ctx) { ctx.target.fields.value += ctx.args.amount; },
  });
  api.handlers.wrap('gameplay:increment.slot',{id:'count-increment'},(ctx,next)=>{
    next();
    ctx.state={...ctx.state,increments:(ctx.state?.increments||0)+1,calls:++calls};
  });
  api.handlers.wrap('builtin:link.transfer',{id:'count-transfer'},(ctx,next)=>{
    next();
    ctx.state={...ctx.state,transfers:(ctx.state?.transfers||0)+1,calls:++calls};
  });
}
"#;

const PLAYER_JS: &str = r#"
export default function install(api) {
  class Counter extends api.builtins.RoomObject {
    get value() { return api.view.field(this,'value'); }
    get secret() { return api.view.field(this,'secret'); }
    get audit() { return api.view.field(this,'audit'); }
    increment(amount) { return api.intents.emit('gameplay:increment',this,{amount}); }
  }
  api.objects.bind('gameplay:counter',Counter);
  api.globals.define('Counter',Counter);
  const queries=api.queries.install('gameplay:counter');
  api.globals.define('FIND_COUNTERS',queries.find);
  api.globals.define('LOOK_COUNTERS',queries.look);
}
"#;

struct Scenario {
    engine: EmbedEngine,
    counters: Vec<String>,
    links: Vec<(String, String)>,
    objects: Vec<String>,
}

impl Scenario {
    fn new(binary: bool, concurrency: usize, reverse: bool) -> anyhow::Result<Self> {
        let root = PackageRoot(std::env::temp_dir().join(format!(
            "kk-mod-multiplayer-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        )));
        for (id, source, player) in [
            ("gameplay", SERVER_JS, Some(PLAYER_JS)),
            (
                "faulty",
                "export default function register(api){api.handlers.wrap('builtin:link.transfer',{id:'fault'},(ctx,next)=>{next();if(ctx.args.amount===25)throw Error('second-room-fault');});}",
                None,
            ),
        ] {
            let dir = root.0.join(id);
            std::fs::create_dir_all(&dir)?;
            let mut manifest = json!({"id":id,"version":"1.0.0","apiVersion":"^1.0.0"});
            if let Some(player) = player {
                manifest["player"] = json!("player.js");
                std::fs::write(dir.join("player.js"), player)?;
            }
            std::fs::write(dir.join("mod.json"), manifest.to_string())?;
            std::fs::write(dir.join("server.js"), source)?;
        }
        let packages = discover_packages(&root.0)?;
        let mut engine = EmbedEngine::new();
        engine.set_kkpv_channel(binary);
        engine.set_player_concurrency(concurrency)?;
        for player in PLAYERS {
            // 低回充使正执行费用可观测，避免桶封顶掩盖重复或缺失计费；不做计时断言。
            engine.set_sandbox_config_for_test(
                player,
                kk_runner::SandboxConfig {
                    cpu_limit: 0.25,
                    initial_bucket: 100.0,
                    ..Default::default()
                },
            );
        }
        engine.create_shard_with_mods(
            &json!({"players":PLAYERS,"rooms":[
                {"name":ROOMS[0],"rcl":6,"owner":PLAYERS[0]},
                {"name":ROOMS[1],"rcl":6,"owner":PLAYERS[1]},
            ]}),
            &packages,
            RegisterOptions {
                fault_disable_threshold: std::num::NonZeroU32::new(2),
                ..Default::default()
            },
        )?;
        let order = if reverse { ["p2", "p1"] } else { PLAYERS };
        engine.set_execution_order_for_test(Some(order.map(str::to_string).to_vec()));
        let mut counters = Vec::new();
        let mut links = Vec::new();
        let mut objects = Vec::new();
        for (index, room) in ROOMS.into_iter().enumerate() {
            let counter = engine.place_mod_object(
                room,
                "gameplay:counter",
                &json!({"owner":PLAYERS[index],"x":10,"y":12,"secret":42+index}),
            )?;
            counters.push(counter.clone());
            objects.push(counter);
            let source = engine.place_object(room,"structure",&json!({"pos":[20,20],"structureType":"link","owner":PLAYERS[index],"store":{"energy":100},"cooldown":0}))?;
            let target = engine.place_object(room,"structure",&json!({"pos":[21,20],"structureType":"link","owner":PLAYERS[index],"store":{"energy":0},"cooldown":0}))?;
            objects.extend([source.clone(), target.clone()]);
            links.push((source, target));
            // 对方的 scout 授予房间视野，以实际玩家投影验证 owner/server-only 裁切。
            objects.push(engine.place_object(room,"creep",&json!({"pos":[30,30],"owner":PLAYERS[1-index],"name":"scout","body":["move"],"ticksToLive":999999}))?);
            let controller = engine
                .find_in_room(room, kk_core::constants::FIND_STRUCTURES)
                .into_iter()
                .find(|object| object["structureType"] == "controller")
                .unwrap();
            objects.push(controller["id"].as_str().expect("控制器 ID").into());
        }
        // 恢复与玩家面安装必须使用启动快照；返回时包目录随 root 删除。
        Ok(Self {
            engine,
            counters,
            links,
            objects,
        })
    }

    fn world(&self) -> Vec<Option<Value>> {
        self.objects
            .iter()
            .map(|id| self.engine.get_object(id))
            .collect()
    }

    fn codes(&self) -> BTreeMap<String, String> {
        PLAYERS.into_iter().enumerate().map(|(index,player)| {
            let own=&self.counters[index];
            let foreign=&self.counters[1-index];
            let (source,target)=&self.links[index];
            let room=ROOMS[index];
            let amount=if index==0 {10} else {25};
            let code=format!(r#"(function(){{
              globalThis.executions=(globalThis.executions||0)+1;
              Memory.executions=globalThis.executions;
              RawMemory.segments[{index}]='{player}:'+globalThis.executions;
              RawMemory.setActiveSegments([{index}]);
              Game.notify('{player}:'+globalThis.executions);
              console.log('{player}:'+globalThis.executions);
              const own=Game.getObjectById('{own}'), foreign=Game.getObjectById('{foreign}');
              const observed=[globalThis.executions,own.value,foreign.value,own.secret,
                foreign.secret===undefined,own.audit===undefined,own instanceof Counter,
                Game.rooms.{room}.find(FIND_COUNTERS).includes(own),
                Game.rooms.{room}.lookForAt(LOOK_COUNTERS,10,12)[0]===own];
              return [...observed,own.increment({increment}),foreign.increment(10),
                Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),{amount})];
            }})()"#,increment=index+2);
            (player.into(),code)
        }).collect()
    }
}

struct Committed {
    world: Vec<Option<Value>>,
    players: BTreeMap<String, PlayerState>,
    mod_state: Value,
    time: u64,
    generation: u64,
    console: BTreeMap<String, Vec<String>>,
}

impl Committed {
    fn capture(scenario: &Scenario) -> Self {
        let engine = &scenario.engine;
        Self {
            world: scenario.world(),
            players: PLAYERS
                .into_iter()
                .map(|player| {
                    (
                        player.into(),
                        engine.player_state_for_test(player).unwrap().clone(),
                    )
                })
                .collect(),
            mod_state: engine.engine_mod_state("gameplay"),
            time: engine.get_game_time(),
            generation: engine.engine_generation(),
            console: PLAYERS
                .into_iter()
                .map(|player| (player.into(), engine.capture_console_logs(player)))
                .collect(),
        }
    }

    fn assert_unchanged(&self, scenario: &Scenario) {
        let engine = &scenario.engine;
        assert_eq!(scenario.world(), self.world);
        assert_eq!(engine.engine_mod_state("gameplay"), self.mod_state);
        assert_eq!(engine.get_game_time(), self.time);
        assert_eq!(engine.engine_generation(), self.generation);
        assert!(engine.has_pending_round());
        assert!(engine.needs_mod_recovery());
        for player in PLAYERS {
            assert_eq!(
                engine.player_state_for_test(player),
                Some(&self.players[player])
            );
            assert_eq!(engine.capture_console_logs(player), self.console[player]);
        }
    }
}

fn assert_published(
    scenario: &mut Scenario,
    before: &Committed,
    runs: &BTreeMap<String, PlayerRun>,
) -> anyhow::Result<()> {
    assert!(!scenario.engine.has_pending_round());
    assert!(!scenario.engine.needs_mod_recovery());
    assert_eq!(scenario.engine.get_game_time(), before.time + 1);
    assert_eq!(scenario.engine.engine_generation(), before.generation + 1);
    assert_eq!(
        scenario.engine.engine_mod_state("gameplay"),
        json!({"increments":2,"transfers":2,"calls":4})
    );
    for (index, player) in PLAYERS.into_iter().enumerate() {
        assert!(runs[player].executed);
        assert_eq!(runs[player].expr["ok"], true);
        assert_eq!(
            runs[player].expr["value"],
            json!([1, 0, 0, 42 + index, true, true, true, true, true, 0, 0, 0])
        );
        assert_eq!(runs[player].console.len(), 1, "缓存 console 只交付一次");
        let state = scenario.engine.player_state_for_test(player).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&state.raw_memory)?,
            json!({"executions":1})
        );
        assert_eq!(state.segments[&(index as u8)], format!("{player}:1"));
        assert_eq!(state.active_request, [index as u8]);
        assert_eq!(state.notifications.len(), 1);
        assert_eq!(state.notifications[0].message, format!("{player}:1"));
        assert_eq!(state.notifications[0].tick, before.time);
        assert!(state.cpu.bucket() < before.players[player].cpu.bucket());
        let (source, target) = &scenario.links[index];
        assert_eq!(
            scenario.engine.get_object(source).unwrap()["store"]["energy"],
            if index == 0 { 90 } else { 75 }
        );
        assert_eq!(
            scenario.engine.get_object(target).unwrap()["store"]["energy"],
            if index == 0 { 9 } else { 24 }
        );
    }
    // 下一轮重新投影：对象值来自统一提交，两个玩家 VM 全局计数都没有重跑。
    let reads = PLAYERS.into_iter().enumerate().map(|(index,player)| {
        (player.into(),format!("(function(){{const own=Game.getObjectById('{}'),foreign=Game.getObjectById('{}');return [globalThis.executions,Memory.executions,RawMemory.segments[{index}],own.value,foreign.value,own.secret,foreign.secret===undefined,own.audit===undefined];}})()",scenario.counters[index],scenario.counters[1-index]))
    }).collect();
    let results = scenario.engine.run_players(&reads)?;
    for (index, player) in PLAYERS.into_iter().enumerate() {
        assert_eq!(
            results[player].expr["value"],
            json!([
                1,
                1,
                format!("{player}:1"),
                index + 2,
                3 - index,
                42 + index,
                true,
                true
            ])
        );
        assert!(results[player].console.is_empty());
        assert_eq!(
            scenario
                .engine
                .player_state_for_test(player)
                .unwrap()
                .notifications
                .len(),
            1
        );
    }
    Ok(())
}

#[test]
fn multi_room_auto_disable_retries_both_cached_players_and_keeps_player_api() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for concurrency in [1, 2] {
            // 参考臂从同一包快照启动，先停用故障处理器，再完成无失败的一轮。
            let mut reference = Scenario::new(binary, 1, false)?;
            reference.engine.disable_mod("faulty")?;
            let reference_runs = reference.engine.run_players(&reference.codes())?;
            let reference_world = reference.world();
            let reference_state = reference.engine.engine_mod_state("gameplay");
            let reference_exprs: BTreeMap<_, _> = reference_runs
                .into_iter()
                .map(|(player, run)| (player, run.expr))
                .collect();
            drop(reference);
            let mut scenario = Scenario::new(binary, concurrency, true)?;
            let before = Committed::capture(&scenario);
            for attempts in 1..=2 {
                let error = scenario.engine.run_players(&scenario.codes()).unwrap_err();
                assert!(error.to_string().contains("second-room-fault"), "{error}");
                before.assert_unchanged(&scenario);
                assert_eq!(
                    scenario
                        .engine
                        .mod_fault_streak()
                        .unwrap()
                        .consecutive_faults,
                    attempts
                );
            }
            assert_eq!(
                scenario.engine.disabled_mods(),
                BTreeSet::from(["faulty".into()])
            );
            let decision = scenario.engine.last_mod_auto_disable().unwrap();
            assert_eq!(decision.trigger.room, "W2N1");
            assert_eq!(decision.trigger.target, scenario.links[1].0);
            scenario.engine.fail_next_mod_rebuild_for_test()?;
            assert!(scenario.engine.run_players(&BTreeMap::new()).is_err());
            before.assert_unchanged(&scenario);
            assert_eq!(scenario.engine.last_mod_auto_disable(), Some(decision));
            // 子集重试必须发布整个缓存轮，不能重派 p1，也不能丢掉 p2 的候选。
            let retry = BTreeMap::from([("p1".into(), "throw Error('不得重跑玩家')".into())]);
            let runs = scenario.engine.run_players(&retry)?;
            assert_eq!(scenario.world(), reference_world);
            assert_eq!(
                scenario.engine.engine_mod_state("gameplay"),
                reference_state
            );
            for player in PLAYERS {
                assert_eq!(runs[player].expr, reference_exprs[player]);
            }
            assert!(scenario.engine.mod_fault_streak().is_none());
            assert_published(&mut scenario, &before, &runs)?;
        }
    }
    Ok(())
}

#[test]
fn multi_room_late_writeback_failure_rolls_back_both_players_and_mod_candidates()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for step in [WritebackStep::RawMemory, WritebackStep::Segments] {
            let mut scenario = Scenario::new(binary, 2, false)?;
            scenario.engine.disable_mod("faulty")?;
            let before = Committed::capture(&scenario);
            scenario.engine.fail_next_writeback_for_test(step);
            assert!(scenario.engine.run_players(&scenario.codes()).is_err());
            before.assert_unchanged(&scenario);
            assert!(scenario.engine.mod_fault_streak().is_none());
            assert!(scenario.engine.last_mod_auto_disable().is_none());
            let runs = scenario.engine.run_players(&BTreeMap::new())?;
            assert_published(&mut scenario, &before, &runs)?;
        }
    }
    Ok(())
}

#[test]
fn continuous_rounds_after_auto_disable_and_writeback_recovery_commit_once() -> anyhow::Result<()> {
    for binary in [false, true] {
        for concurrency in [1, 2] {
            let mut scenario = Scenario::new(binary, concurrency, true)?;
            let initial_time = scenario.engine.get_game_time();
            let initial_generation = scenario.engine.engine_generation();
            let mut calls_since_rebuild = 0;
            for round in 1..=24 {
                let codes = if round == 1 {
                    scenario.codes()
                } else {
                    PLAYERS.into_iter().enumerate().map(|(index,player)| {
                        (player.into(),format!(r#"(function(){{
                          globalThis.executions++;
                          const previous=Memory.executions;
                          Memory.executions=globalThis.executions;
                          RawMemory.segments[{index}]='{player}:'+globalThis.executions;
                          RawMemory.setActiveSegments([{index}]);
                          Game.notify('{player}:'+globalThis.executions);
                          console.log('{player}:'+globalThis.executions);
                          const own=Game.getObjectById('{own}'),foreign=Game.getObjectById('{foreign}');
                          return [globalThis.executions,previous,own.value,foreign.value,
                            own instanceof Counter,foreign.secret===undefined,own.audit===undefined,
                            own.increment({increment}),foreign.increment(10)];
                        }})()"#,own=scenario.counters[index],foreign=scenario.counters[1-index],increment=index+2))
                    }).collect()
                };
                let before = Committed::capture(&scenario);
                let recovery = matches!(round, 1 | 9 | 17);
                if round == 1 {
                    for _ in 0..2 {
                        let error = scenario.engine.run_players(&codes).unwrap_err();
                        assert!(error.to_string().contains("second-room-fault"));
                        before.assert_unchanged(&scenario);
                    }
                    assert_eq!(
                        scenario.engine.disabled_mods(),
                        BTreeSet::from(["faulty".into()])
                    );
                } else if recovery {
                    scenario.engine.fail_next_writeback_for_test(if round == 9 {
                        WritebackStep::RawMemory
                    } else {
                        WritebackStep::Segments
                    });
                    assert!(scenario.engine.run_players(&codes).is_err());
                    before.assert_unchanged(&scenario);
                }
                let runs = if recovery {
                    calls_since_rebuild = 0;
                    // 空请求重试整个缓存轮；下一正常轮还会检查两个 VM 的全局执行序号。
                    scenario.engine.run_players(&BTreeMap::new())?
                } else {
                    scenario.engine.run_players(&codes)?
                };
                calls_since_rebuild += if round == 1 { 4 } else { 2 };
                assert_eq!(scenario.engine.get_game_time(), initial_time + round);
                assert_eq!(
                    scenario.engine.engine_generation(),
                    initial_generation + round
                );
                assert!(!scenario.engine.has_pending_round());
                assert!(!scenario.engine.needs_mod_recovery());
                assert_eq!(
                    scenario.engine.engine_mod_state("gameplay"),
                    json!({"increments":round*2,"transfers":2,"calls":calls_since_rebuild})
                );
                for (index, player) in PLAYERS.into_iter().enumerate() {
                    let run = &runs[player];
                    assert!(run.executed);
                    assert_eq!(run.expr["ok"], true);
                    if round > 1 {
                        assert_eq!(
                            run.expr["value"],
                            json!([
                                round,
                                round - 1,
                                (round - 1) * (index as u64 + 2),
                                (round - 1) * (3 - index as u64),
                                true,
                                true,
                                true,
                                0,
                                0
                            ])
                        );
                    }
                    assert_eq!(run.console.len(), 1);
                    let state = scenario.engine.player_state_for_test(player).unwrap();
                    assert_eq!(
                        serde_json::from_str::<Value>(&state.raw_memory)?,
                        json!({"executions":round})
                    );
                    assert_eq!(state.segments[&(index as u8)], format!("{player}:{round}"));
                    assert_eq!(state.active_request, [index as u8]);
                    assert_eq!(state.notifications.len(), round as usize);
                    assert_eq!(
                        state.notifications.last().unwrap().message,
                        format!("{player}:{round}")
                    );
                    assert_eq!(
                        state.notifications.last().unwrap().tick,
                        initial_time + round - 1
                    );
                    assert_eq!(
                        scenario
                            .engine
                            .get_object(&scenario.counters[index])
                            .unwrap()["fields"]["value"],
                        round * (index as u64 + 2)
                    );
                    if recovery {
                        assert!(
                            state.cpu.bucket() < before.players[player].cpu.bucket(),
                            "恢复发布需扣原执行费用"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
