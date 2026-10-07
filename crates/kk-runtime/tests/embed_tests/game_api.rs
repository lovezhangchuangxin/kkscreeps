//! 玩家面 Game/RawMemory API 域：notify/cpu、segments 与 visual 序列化 e2e。

use super::*;

pub(super) fn notify_and_cpu_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;

    // notify：无参/带 groupInterval 均 OK；每 tick 20 帽（第 20 条 OK、21 起
    // ERR_FULL）——GAME-NOTIFY-001/002 同形
    let run = e.run_player(
        "p1",
        r#"[ typeof Game.notify,
             Game.notify('hello'),
             Game.notify('hello', 5),
             Game.notify() ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "function");
    assert_eq!(v[1], 0);
    assert_eq!(v[2], 0);
    assert_eq!(v[3], 0, "无参亦 OK（message undefined 落库容空串）");

    let run = e.run_player(
        "p1",
        r#"(function () {
             const codes = [];
             for (let i = 0; i < 25; i++) codes.push(Game.notify('n' + i, 10));
             return [codes[19], codes[20], codes[24]];
           })()"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 0, "第 20 条（索引 19）OK");
    assert_eq!(v[1], -8, "第 21 条 ERR_FULL");
    assert_eq!(v[2], -8);
    assert_eq!(
        e.player_notifications_for_test("p1").len(),
        23,
        "tick1 的 3 条+tick2 帽内 20 条（i=0..19 全过）落账"
    );

    // 帽随 tick 重置（beginTick 重建 intents list）
    e.tick(1)?;
    let run = e.run_player("p1", r#"Game.notify('fresh tick')"#)?;
    assert_eq!(run.expr["value"], 0, "新 tick 帽重置");
    e.tick(1)?;
    let records = e.player_notifications_for_test("p1");
    assert_eq!(records.len(), 24);
    assert_eq!(records[23].message, "fresh tick");
    assert_eq!(records[23].group_interval, None);
    // tick1 三条：notify('hello')（无间隔）/notify('hello', 5)/notify()
    //——undefined 键 JSON 序列化自然丢弃
    assert_eq!(records[0].message, "hello");
    assert_eq!(records[0].group_interval, None);
    assert_eq!(records[1].message, "hello");
    assert_eq!(records[1].group_interval, Some(5));
    assert_eq!(records[2].message, "");

    // cpu 三面：cpuLimit=tickLimit；getHeapStatistics 真数值（limit>0 且
    // used∈[0,limit]——CPU-HEAP-001 同形）；halt 硬停（后续代码不可达、
    // 无 console 错误、tick 完成、下一 tick 可用）
    let run = e.run_player(
        "p1",
        r#"[ Game.cpuLimit === Game.cpu.tickLimit,
             Game.cpu.getHeapStatistics().heap_size_limit > 0,
             Game.cpu.getHeapStatistics().used_heap_size >= 0
               && Game.cpu.getHeapStatistics().used_heap_size
                 <= Game.cpu.getHeapStatistics().heap_size_limit,
             typeof Game.cpu.getHeapStatistics().externally_allocated_size ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(
        v[0], true,
        "cpuLimit=tickLimit（官方 runtimeData.cpu 同值）"
    );
    assert_eq!(v[1], true, "heap_size_limit 真数值");
    assert_eq!(v[2], true, "used 在界内");
    assert_eq!(
        v[3], "number",
        "snake_case 键面（overmind/tooangel 消费形）"
    );

    let run = e.run_player(
        "p1",
        r#"
        console.log('before-halt');
        Game.cpu.halt();
        console.log('after-halt');
        throw new Error('unreachable');
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("before-halt")),
        "{:?}",
        run.console
    );
    assert!(
        !run.console.iter().any(|l| l.contains("after-halt")),
        "halt 后代码不可达"
    );
    // halt 除了返回运行错误，还应输出 CPU halted 错误日志。
    assert!(
        run.console.iter().any(|l| l.contains("CPU halted")),
        "CPU halted 错误行：{:?}",
        run.console
    );
    assert!(
        !run.console.iter().any(|l| l.contains("unreachable")),
        "halt 后玩家代码不可达：{:?}",
        run.console
    );
    // halt 跳过本轮计费，提交前后的 CPU 桶净变动应为 0。
    let before = e.player_bucket_for_test("p1")?;
    let run = e.run_player("p1", r#"Game.cpu.halt(); console.log('nope');"#)?;
    assert!(run.console.iter().any(|l| l.contains("CPU halted")));
    let after = e.player_bucket_for_test("p1")?;
    assert_eq!(
        before, after,
        "halt tick 零计费——桶净变动 0（正常 tick 有 limit−used 回充差）"
    );
    // halt 后沙箱健康：下一 tick 正常执行
    e.tick(1)?;
    let run = e.run_player("p1", r#"1 + 1"#)?;
    assert_eq!(run.expr["value"], 2, "halt 后沙箱可复用");
    Ok(())
}

pub(super) fn segments_and_visuals_e2e_impl() -> anyhow::Result<()> {
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

    // 空数组清除活动段请求；与公开段空数组不发出更新的行为分别检查。
    e.run_player("p1", r#"RawMemory.setActiveSegments([]); 'ok'"#)?;
    let run = e.run_player("p1", r#"[ Object.keys(RawMemory.segments).length ]"#)?;
    assert_eq!(
        run.expr["value"][0],
        json!(0),
        "空数组清空窗口（下 tick segments 为空对象）"
    );

    // Memory.toJSON 在段结果收集前执行，因此回调中的段写入和请求应在本轮保存。
    e.run_player(
        "p1",
        r#"Memory.toJSON = function () {
             RawMemory.segments[7] = 'from-memory';
             RawMemory.setActiveSegments([7]);
             return { saved: true };
           };
           'sent'"#,
    )?;
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.segments[7], RawMemory.get().indexOf('saved') !== -1 ]"#,
    )?;
    assert_eq!(
        run.expr["value"],
        json!(["from-memory", true]),
        "toJSON 回调写段+请求同轮生效、Memory 已保存"
    );

    // toJSON 回调内的超限段 → 整轮失败（修复前：段已打包、违例不可见 → 假成功）
    let run = e.run_player(
        "p1",
        r#"Memory.toJSON = function () {
             RawMemory.segments[8] = 'x'.repeat(100 * 1024 + 1);
             return {};
           };
           'sent'"#,
    )?;
    assert_eq!(
        run.expr["ok"],
        json!(false),
        "toJSON 内超限写段 tick 尾校验仍拦截（官方序下回调写先于段回收）"
    );

    // 段 getter 在类型检查、长度检查和取最终值时各读取一次，写入的是第三次值。
    // 用 Object.entries 提前快照会错误地保存第一次值。
    e.run_player(
        "p1",
        r#"RawMemory.setActiveSegments([5]);
           let n = 0;
           Object.defineProperty(RawMemory.segments, '5', {
             enumerable: true, configurable: true,
             get() { return n++ === 0 ? 'first' : 'later'; },
           });
           'sent'"#,
    )?;
    let run = e.run_player("p1", r#"[ RawMemory.segments[5] ]"#)?;
    assert_eq!(
        run.expr["value"][0],
        json!("later"),
        "入库值=末次读（官方三读序；快照实现会存 first）"
    );

    // 段数量超过上限时，应先报数量错误，而不是运行段 getter 并抛出其异常。
    let run = e.run_player(
        "p1",
        r#"RawMemory.setActiveSegments([0,1,2,3,4,5,6,7,8,9]);
           for (let i = 0; i < 11; i++) {
             Object.defineProperty(RawMemory.segments, '' + i, {
               enumerable: true, configurable: true,
               get() { throw new Error('getter-first'); },
             });
           }
           'sent'"#,
    )?;
    assert_eq!(run.expr["ok"], json!(false), "11 段违例整轮失败");
    assert!(
        run.expr["error"]
            .as_str()
            .is_some_and(|s| s.contains("Cannot save more than 10")),
        "数量门优先（快照实现会先报 getter-first）：{}",
        run.expr["error"]
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

    // 已授权但未写过的外部段返回 data=null，活动段则返回空字符串。
    e.run_player("p2", r#"RawMemory.setPublicSegments([1, 3]); 'ok'"#)?;
    e.run_player("p1", r#"RawMemory.setActiveForeignSegment('p2', 3); 'ok'"#)?;
    let run = e.run_player(
        "p1",
        r#"[ RawMemory.foreignSegment.data === null, RawMemory.foreignSegment.id ]"#,
    )?;
    assert_eq!(run.expr["value"][0], json!(true), "未写过公开槽 data=null");
    assert_eq!(run.expr["value"][1], json!(3), "显式 id 交付");

    // foreignSegment 的属性可枚举、可配置、可写，严格模式赋值也应成功。
    let run = e.run_player(
        "p1",
        r#"[ (() => { const d = Object.getOwnPropertyDescriptor(RawMemory, 'foreignSegment');
                   return d ? [d.writable, d.enumerable, d.configurable] : 'absent'; })(),
             (() => { 'use strict'; try { RawMemory.foreignSegment = 1; return 'assigned'; }
                   catch (e) { return 'threw'; } })() ]"#,
    )?;
    assert_eq!(
        run.expr["value"][0],
        json!([true, true, true]),
        "foreignSegment 普通赋值挂载（官方描述符面）"
    );
    assert_eq!(
        run.expr["value"][1],
        json!("assigned"),
        "严格模式赋值不抛（官方同形）"
    );

    // 检查 visual 绘制结果、大小、共享缓冲区和长度上限。
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

    // 检查 CostMatrix 序列化往返、克隆独立性和数值范围限制。
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
