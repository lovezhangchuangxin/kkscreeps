//! M2B 扩面 P6 收尾 e2e：RawMemory segments 全链（激活窗口/写回持久/
//! 10 段与 100KB 上限/foreign 授权-默认-撤回-清除）、RoomVisual/MapVisual
//! （绘制记账/尺寸账计/上限拒绝/sharedBuffer）、CostMatrix 序列化往返。
//! 官方对照：driver/lib/runtime/runtime.js:122-200、make.js:250-267、
//! game/console.js:57-108、rooms.js:1144-1218、map.js:275-374、
//! game/path-finder.js:18-47。

use super::*;

pub(super) fn m2b_closing_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" },
                  { "name": "W2N1", "rcl": 3, "owner": "p2" }],
    }))?;

    // ---- segments：激活请求 → 窗口出现（未写过物化空串，RAWMEMORY-003）
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.setActiveSegments([5]), 5 in RawMemory.segments ]"#,
    )?;
    assert_eq!(
        run.expr["value"],
        json!([serde_json::Value::Null, false]),
        "同 tick 不替换窗口（官方 setActiveSegments 无返回值）"
    );

    let run = e.run_player(
        "p1",
        r#"[ 5 in RawMemory.segments, RawMemory.segments[5] ]"#,
    )?;
    assert_eq!(
        run.expr["value"],
        json!([true, ""]),
        "下 tick 窗口含请求槽（未写过物化空串）"
    );

    // 写回持久（RAWMEMORY-004/005）+ 10 段上限 + 越界 id throw（002）
    let run = e.run_player(
        "p1",
        r#"[ (RawMemory.segments[5] = 'test-data'),
             (() => { try { RawMemory.setActiveSegments([0,1,2,3,4,5,6,7,8,9,10]); return 'ok'; } catch (e) { return e.message; } })(),
             (() => { try { RawMemory.setActiveSegments([100]); return 'ok'; } catch (e) { return e.message; } })() ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!("test-data"));
    assert_eq!(
        v[1],
        json!("Only 10 memory segments can be active at the same time"),
        "11 段 throw（官方文案）"
    );
    assert_eq!(
        v[2],
        json!("\"100\" is not a valid segment ID"),
        "id 100 throw"
    );

    // 100KB+1 写回整 tick 失败（002:segmentSize——runtime error，写不落）
    let run = e.run_player(
        "p1",
        r#"RawMemory.setActiveSegments([5, 7]);
           RawMemory.segments[7] = 'x'.repeat(100 * 1024 + 1);
           'sent'"#,
    )?;
    assert_eq!(
        run.expr["ok"],
        json!(false),
        "写本身当 tick 不抛、tick 尾校验失败整轮 runtime error（官方 make throw 同义）"
    );
    let run = e.run_player("p1", r#"[ RawMemory.segments[7] ]"#)?;
    assert_eq!(
        run.expr["value"][0],
        serde_json::Value::Null,
        "超限 tick 整体失败——写不落（窗口里 7 不存在）"
    );

    // 恰 100KB 通过
    e.run_player(
        "p1",
        r#"RawMemory.setActiveSegments([5, 9]);
           RawMemory.segments[9] = 'y'.repeat(100 * 1024);
           'sent'"#,
    )?;
    let run = e.run_player("p1", r#"[ RawMemory.segments[9].length ]"#)?;
    assert_eq!(run.expr["value"][0], json!(100 * 1024), "恰 100KB 落库");

    // 空数组清空活动集（评审 1/2 P1 清偿：官方 runtime.js:287 无守卫 emit、
    // [] truthy 经 make 侧落库清空——与 public 的 [] no-op 是两个形状）
    e.run_player("p1", r#"RawMemory.setActiveSegments([]); 'ok'"#)?;
    let run = e.run_player("p1", r#"[ Object.keys(RawMemory.segments).length ]"#)?;
    assert_eq!(
        run.expr["value"][0],
        json!(0),
        "空数组清空窗口（下 tick segments 为空对象）"
    );

    // ---- foreign：发布/默认回退/显式 id 未授权/撤回/清除/未知名
    e.run_player(
        "p2",
        r#"RawMemory.setActiveSegments([1]);
           RawMemory.segments[1] = 'shared-data';
           RawMemory.setPublicSegments([1]);
           RawMemory.setDefaultPublicSegment(1);
           'ok'"#,
    )?;
    e.run_player("p1", r#"RawMemory.setActiveForeignSegment('p2'); 'ok'"#)?;
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.foreignSegment ? [RawMemory.foreignSegment.username,
                                          RawMemory.foreignSegment.id,
                                          RawMemory.foreignSegment.data] : null ]"#,
    )?;
    assert_eq!(
        run.expr["value"][0],
        json!(["p2", 1, "shared-data"]),
        "FOREIGN-002：默认段回退交付 username/id/data 三字段"
    );

    // 撤回：p2 换公开集（空列表是官方 no-op——须换 id）
    e.run_player("p2", r#"RawMemory.setPublicSegments([8]); 'ok'"#)?;
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.foreignSegment === undefined ? 'undefined' : 'defined' ]"#,
    )?;
    assert_eq!(
        run.expr["value"][0],
        json!("undefined"),
        "FOREIGN-008：默认段残留不构成授权（公开集撤回即断供）"
    );

    // 清除请求（FOREIGN-006）与未知名（FOREIGN-007）
    e.run_player("p2", r#"RawMemory.setPublicSegments([1]); 'ok'"#)?;
    e.run_player("p1", r#"RawMemory.setActiveForeignSegment('p2'); 'ok'"#)?;
    e.run_player("p1", r#"RawMemory.setActiveForeignSegment(null); 'ok'"#)?;
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.foreignSegment === undefined ? 'undefined' : 'defined',
             (() => { try { RawMemory.setActiveForeignSegment('Nobody-' + Math.random()); return 'ok'; } catch (e) { return 'threw'; } })() ]"#,
    )?;
    assert_eq!(run.expr["value"][0], json!("undefined"), "null 清除生效");
    assert_eq!(
        run.expr["value"][1],
        json!("ok"),
        "未知名不 throw（下 tick 无交付）"
    );

    // 未写过的公开槽 data=null（官方 data.js:258 hget 缺失即 null——与活动槽
    // :251 物化空串刻意不对称；评审 1 P2-3/评审 2 P2-1 清偿）
    e.run_player("p2", r#"RawMemory.setPublicSegments([1, 3]); 'ok'"#)?;
    e.run_player("p1", r#"RawMemory.setActiveForeignSegment('p2', 3); 'ok'"#)?;
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.foreignSegment.data === null, RawMemory.foreignSegment.id ]"#,
    )?;
    assert_eq!(run.expr["value"][0], json!(true), "未写过公开槽 data=null");
    assert_eq!(run.expr["value"][1], json!(3), "显式 id 交付");

    // ---- visuals：绘制记账/尺寸/sharedBuffer/上限拒绝（VISUAL 族）
    let run = e.run_player(
        "p1",
        r#"[ (() => { const v = new RoomVisual('W1N1');
                   v.circle(10, 10, { radius: 0.5 }).line(1, 1, 2, 2).rect(5, 5, 1, 1)
                    .poly([[0,0],[1,1]]).text('hi', 3, 3);
                   return v.getSize() > 0; })(),
             new RoomVisual('W1N1').getSize(),
             (() => { const a = new RoomVisual('W8N8'); const b = new RoomVisual('W8N8');
                   a.circle(1, 1); return b.getSize() > 0; })(),
             Game.map.visual.circle(new RoomPosition(10, 10, 'W1N1'), { radius: 2 }).getSize() > 0 ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(true), "五种绘制记账");
    assert!(
        v[1].as_i64().unwrap_or(0) > 0,
        "同 tick 同房新实例共享同一缓冲（首块的绘制可见）"
    );
    assert_eq!(v[2], json!(true), "sharedBuffer：同房两实例共一缓冲");
    assert_eq!(v[3], json!(true), "MapVisual 世界坐标绘制记账");

    // 上限拒绝 + clear 恢复（VISUAL-SIZE-003/004——房 500KB）
    let run = e.run_player(
        "p1",
        r#"[ (() => { const v = new RoomVisual('W7N7');
                   try { for (let i = 0; i < 4000; i++) v.text('0123456789'.repeat(13), i % 50, i / 50 | 0); return 'no-throw'; }
                   catch (e) { return /500 KB/.test(e.message) ? 'rejected' : e.message; } })(),
             (() => { const v = new RoomVisual('W7N7'); v.clear(); return v.getSize(); })() ]"#,
    )?;
    assert_eq!(
        run.expr["value"][0],
        json!("rejected"),
        "超 500KB throw（官方文案）"
    );
    assert_eq!(run.expr["value"][1], json!(0), "clear 恢复");

    // ---- CostMatrix：序列化往返/clone 独立/钳制（COSTMATRIX 族 + UNDOC-_bits）
    let run = e.run_player(
        "p1",
        r#"[ (() => { const m = new PathFinder.CostMatrix();
                   m.set(3, 4, 42); m.set(5, 5, 999);
                   const s = m.serialize();
                   const r = PathFinder.CostMatrix.deserialize(s);
                   return [s.length, r.get(3, 4), r.get(5, 5), r.get(0, 0)]; })(),
             (() => { const m = new PathFinder.CostMatrix(); m.set(1, 1, 7);
                   const c = m.clone(); c.set(1, 1, 9);
                   return [m.get(1, 1), c.get(1, 1)]; })(),
             new PathFinder.CostMatrix()._bits.length ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(
        v[0],
        json!([625, 42, 255, 0]),
        "625×uint32 打包往返 + 钳 0..255"
    );
    assert_eq!(v[1], json!([7, 9]), "clone 独立");
    assert_eq!(v[2], json!(2500), "_bits 公开面（x*50+y 索引）");

    Ok(())
}
