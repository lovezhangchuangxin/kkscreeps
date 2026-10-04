//! 玩家寻路通道的局部微基准；独立进程运行，不充当正式端到端性能验收。
use kk_runner::{ModuleTable, Sandbox, SandboxConfig};
use serde_json::json;

#[test]
#[ignore = "release 微基准；不要与其他负载同时采集"]
fn player_pathfinder_callback_cost() -> anyhow::Result<()> {
    let mut sandbox = Sandbox::new(SandboxConfig {
        max_per_tick: 10_000.0,
        ..SandboxConfig::default()
    })?;
    let mut modules = ModuleTable::new();
    modules.insert_js("main", "module.exports.loop=function(){};");
    sandbox.set_modules(modules)?;
    sandbox.begin_tick(&json!({"time":1,"user":"p1","rooms":{
        "W1N1":{"name":"W1N1","terrain":vec![0u8;2500]}},"objects":[
            {"id":"viewer","type":"creep","name":"viewer","room":"W1N1",
             "x":5,"y":5,"owner":"p1","body":["move"]}]}))?;
    let cases = [
        ("terrain", "PathFinder.search(from,to,{maxRooms:1})"),
        (
            "callback_none",
            "PathFinder.search(from,to,{maxRooms:1,roomCallback:()=>undefined})",
        ),
        (
            "callback_matrix",
            "PathFinder.search(from,to,{maxRooms:1,roomCallback:()=>matrix})",
        ),
        (
            "room_grid",
            "Game.rooms.W1N1.findPath(from,to,{ignoreCreeps:true})",
        ),
    ];
    let mut samples = Vec::new();
    // 热身后五轮轮换顺序；1000 次搜索共享一次表达式/op 结果写回。
    for round in 0..6 {
        for index in 0..cases.len() {
            let (name, search) = cases[(index + round) % cases.len()];
            let expr = format!(
                "(() => {{const from=new RoomPosition(10,25,'W1N1'),to=new RoomPosition(30,25,'W1N1'),matrix=new PathFinder.CostMatrix();let sum=0;for(let i=0;i<1000;i++){{const r={search};sum+=r.path?r.path.length:r.length;}}return sum;}})()"
            );
            let start = std::time::Instant::now();
            let result = sandbox.run_expr(&expr)?;
            let micros_per_search = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(result["value"], 20000, "{name} 路径应为 20 步");
            if round != 0 {
                samples
                    .push(json!({"round":round,"case":name,"micros_per_search":micros_per_search}));
            }
        }
    }
    println!(
        "PATHFINDER_PLAYER_BENCH={}",
        serde_json::to_string(&samples)?
    );
    sandbox.end_tick()?;
    Ok(())
}
