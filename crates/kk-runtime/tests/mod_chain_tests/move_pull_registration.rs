//! 拉拽登记链：输入完整性、配对否决语义（等同从未登记）、拒绝回退、
//! 能力门、组合与故障停用回退。
//!
//! 场景与准入测试同源：puller(move) 在 (10,10) 让位到 (10,9)，cart(work)
//! 无 MOVE 部件在 (10,11) 经 move(puller) 请求拉者原格——配对完好时
//! 原生即获准，配对被否决等同从未登记，cart 原地。
use std::collections::BTreeSet;

use super::*;
use harness::{creep_at, observe_pkg, pos_in};

fn pair(e: &mut EmbedEngine) -> anyhow::Result<(String, String)> {
    // 拉者带一件 work 使自重为 1：自有平原成本 2 与单 MOVE 件衰减 2
    // 相抵，配对完好时被拉者的转移疲劳（+2）才可观测。
    let puller = creep_at(e, "W1N1", "puller", "p1", 10, 10, &["move", "work"])?;
    let cart = creep_at(e, "W1N1", "cart", "p1", 10, 11, &["work"])?;
    Ok((puller, cart))
}

const DRAG: &str = "Game.creeps.puller.move(TOP);Game.creeps.cart.move(Game.creeps.puller)";
/// 无 pull 意图与否决配对在下游完全一致：cart 都不动。
const PULL_DRAG: &str = "Game.creeps.puller.pull(Game.creeps.cart);Game.creeps.puller.move(TOP);Game.creeps.cart.move(Game.creeps.puller)";

const ANCHOR: &str = "api.handlers.replace('builtin:room.movePullRegistration',{id:'anchor'},ctx=>{
  for(const p of ctx.args.pulls){
    if(!ctx.pull.isDenied(p.pullerId))ctx.pull.deny(p.pullerId);
  }
  ctx.state={applied:(ctx.state?.applied||0)+1,count:ctx.args.pulls.length};
});";

#[test]
fn pull_input_is_complete_and_native_contrast() -> anyhow::Result<()> {
    // 初读校验字段形态；否决后 cart 原地——原生对照里配对完好、
    // cart 免除部件门槛获准。
    let probe = package(
        "api.handlers.replace('builtin:room.movePullRegistration',{id:'probe'},ctx=>{
        for(const p of ctx.args.pulls){
          if(typeof p.pullerId!=='string'||typeof p.targetId!=='string'
            ||typeof p.pullerX!=='number'||typeof p.pullerY!=='number'
            ||typeof p.targetX!=='number'||typeof p.targetY!=='number'
            ||typeof p.pullerOwner!=='string'||typeof p.targetOwner!=='string')throw Error('配对要素');
        }
        const first=ctx.args.pulls[0];
        if(first.pullerX!==10||first.pullerY!==10||first.targetX!==10||first.targetY!==11)throw Error('配对坐标');
        if(!ctx.pull.isDenied(first.pullerId))ctx.pull.deny(first.pullerId);
        ctx.state={applied:(ctx.state?.applied||0)+1,count:ctx.args.pulls.length};
      });",
    );
    let mut e = engine(&[("alpha", probe)], false)?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9), "拉者让位不受影响");
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 11), "配对被否决：cart 原地");
    assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1,"count":1}));
    // 无配对的 tick 不进入 JS。
    e.run_player("p1", "Memory.tick=Game.time")?;
    assert_eq!(
        e.engine_mod_state("alpha"),
        json!({"applied":1,"count":1}),
        "计数不重复"
    );
    let denied_fatigue = e.get_object(&puller).unwrap()["fatigue"].as_i64().unwrap();
    assert_eq!(
        e.get_object(&cart).unwrap()["fatigue"].as_i64(),
        Some(0),
        "被否决配对无疲劳转移"
    );

    // 原生对照：无策略包（仅观察计数）时配对完好、cart 获准；
    // 被拉者的移动疲劳转移给拉者（对照否决路径无转移）。
    let mut e = engine(
        &[("watch", observe_pkg("builtin:room.movePullRegistration"))],
        false,
    )?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 10), "原生配对拉拽获准");
    let native_fatigue = e.get_object(&puller).unwrap()["fatigue"].as_i64().unwrap();
    assert!(
        native_fatigue > denied_fatigue,
        "配对完好时被拉者疲劳转移给拉者：{native_fatigue} > {denied_fatigue}"
    );
    Ok(())
}

#[test]
fn denied_pair_matches_no_pull_intent_baseline() -> anyhow::Result<()> {
    // 否决配对与"从未提交 pull 意图"逐对象一致。
    let anchor = package(ANCHOR);
    let mut e = engine(&[("anchor", anchor)], false)?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;

    let mut base = engine(&[("anchor", package(ANCHOR))], false)?;
    let (base_puller, base_cart) = pair(&mut base)?;
    base.run_player("p1", DRAG)?;
    assert_eq!(
        pos_in(&e, "W1N1", &puller),
        pos_in(&base, "W1N1", &base_puller)
    );
    assert_eq!(pos_in(&e, "W1N1", &cart), pos_in(&base, "W1N1", &base_cart));
    Ok(())
}

#[test]
fn pull_decline_falls_back_to_native() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:room.movePullRegistration',{id:'decline'},ctx=>{
        ctx.pull.deny(ctx.args.pulls[0].pullerId);
        return api.decline('full');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;
    // 拒绝丢弃全部否决：与原生一致。
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 10), "拒绝回退原生配对");
    assert_eq!(e.engine_mod_state("alpha"), Value::Null, "拒绝不提交 state");
    Ok(())
}

#[test]
fn pull_composes_with_precheck_views() -> anyhow::Result<()> {
    // 组合：拉拽否决先行，预检视野里被否决配对的 pulled/pulling
    // 均为假（等同从未登记）；预检自身否决拉者请求的既有语义不变。
    let anchor = package(ANCHOR);
    let viewer = package(
        "api.handlers.before('builtin:room.movePrecheck',{id:'viewer'},ctx=>{
        const cart=ctx.args.requests.find(r=>r.originY===11);
        const puller=ctx.args.requests.find(r=>r.originY===10);
        if(cart.pulled||puller.pulling)throw Error('被否决配对不应出现在预检视野');
        ctx.state={applied:(ctx.state?.applied||0)+1};
      });",
    );
    let mut e = engine(&[("anchor", anchor), ("viewer", viewer)], false)?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 11), "两级组合：cart 仍原地");
    assert_eq!(e.engine_mod_state("anchor"), json!({"applied":1,"count":1}));
    assert_eq!(e.engine_mod_state("viewer"), json!({"applied":1}));
    Ok(())
}

#[test]
fn pull_composes_with_admission_prefill() -> anyhow::Result<()> {
    // 准入槽读到的原生预填已是 post-deny 形态：anchor 否决配对后，
    // 准入视野里 cart 的 pulledBy 为 null；准入覆盖仍可放行 cart
    // （放行只解除门槛，配对已不存在）。
    let anchor = package(ANCHOR);
    let viewer = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'viewer'},ctx=>{
        const cart=ctx.args.movers.find(m=>m.originY===11);
        if(cart.pulledBy!==null)throw Error('被否决配对不应出现在准入预填');
        ctx.admission.setAdmitted(cart.id,true);
        ctx.state={applied:(ctx.state?.applied||0)+1};
      });",
    );
    let mut e = engine(&[("anchor", anchor), ("viewer", viewer)], false)?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9));
    assert_eq!(
        pos_in(&e, "W1N1", &cart),
        (10, 10),
        "准入放行解除门槛：cart 获准但无疲劳转移"
    );
    assert_eq!(e.engine_mod_state("anchor"), json!({"applied":1,"count":1}));
    assert_eq!(e.engine_mod_state("viewer"), json!({"applied":1}));
    Ok(())
}

#[test]
fn pull_decline_discards_before_layer_state() -> anyhow::Result<()> {
    // before 层写 state 后中心拒绝：暂存 state 必须丢弃（与预检/竞争/
    // 准入/障碍槽同语义），否则同输入重试会双计。
    let source = package(
        "api.handlers.before('builtin:room.movePullRegistration',{id:'note'},ctx=>{
        ctx.state={seen:(ctx.state?.seen||0)+1};
      });
        api.handlers.replace('builtin:room.movePullRegistration',{id:'gate'},ctx=>{
        return api.decline('full');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (puller, cart) = pair(&mut e)?;
    e.run_player("p1", PULL_DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9), "拒绝回退原生配对");
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 10));
    assert_eq!(e.engine_mod_state("alpha"), Value::Null, "暂存 state 丢弃");
    Ok(())
}

#[test]
fn pull_abilities_are_gated_to_pull_slot() -> anyhow::Result<()> {
    // ctx.pull 只在拉拽登记槽有效：在准入槽处理器里调用走宿主能力门
    // 故障，不能被 catch+decline 掩盖，整轮候选失败且世界不变。
    let source = package(
        "api.handlers.replace('builtin:creep.moveAdmission',{id:'wrong'},ctx=>{
        try{ctx.pull.deny(ctx.args.movers[0].id)}catch(e){}
        return api.decline('hide');
      });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let (puller, cart) = pair(&mut e)?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", PULL_DRAG).is_err(), "能力门故障整轮失败");
    assert_eq!(e.world_snapshot(), before, "世界不变");
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 10));
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 11));
    Ok(())
}

#[test]
fn pull_fault_disable_falls_back_to_native_pairing() -> anyhow::Result<()> {
    // 处理器连续故障跨过阈值后整包停用，槽回退 native 配对；停用
    // 不卸载能力面，晚期重试仍恰一次。
    let source = package(
        "api.handlers.replace('builtin:room.movePullRegistration',{id:'faulty'},ctx=>{
        ctx.pull.deny(ctx.args.pulls[0].pullerId);
        throw Error('pull fault');
      });",
    );
    let options = RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(2),
        ..Default::default()
    };
    let mut e = engine_with_options(&[("alpha", source)], false, &[], options)?;
    let (puller, cart) = pair(&mut e)?;
    for _ in 0..2 {
        assert!(e.run_player("p1", PULL_DRAG).is_err(), "故障轮整轮失败");
        e.discard_pending_round();
        e.rebuild_mod_realm()?;
    }
    assert_eq!(
        e.disabled_mods(),
        ["alpha".to_string()].into_iter().collect::<BTreeSet<_>>()
    );
    e.run_player("p1", PULL_DRAG)?;
    assert_eq!(pos_in(&e, "W1N1", &puller), (10, 9), "停用后回退原生配对");
    assert_eq!(pos_in(&e, "W1N1", &cart), (10, 10));
    Ok(())
}
