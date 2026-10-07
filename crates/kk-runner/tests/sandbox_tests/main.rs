//! 验证玩家 API、生命周期、预算候选和中断后沙箱复用。
//!
//! 所有场景在一个测试函数中顺序执行，最后核对进程级存活计数与销毁顺序计数，
//! 避免并行场景互相影响。

use std::path::PathBuf;

use serde_json::json;

use kk_runner::{ModuleTable, Sandbox, SandboxConfig, TickOutcome, TickPlan, live_sandbox_count};

mod constant_gate_forms;
mod kkpv_channel;
mod mod_player_api;
mod pathfinder_family;
mod prelude_forms;
mod resource_host_and_nearheap;
mod room_forms;
mod store_spawn_and_viewer;
mod watchdog_and_memory;
// 子模块提供场景函数，由唯一的顶层测试串行调用；不要将它们改成并行测试。
use constant_gate_forms::*;
use kkpv_channel::*;
use mod_player_api::*;
use pathfinder_family::*;
use prelude_forms::*;
use resource_host_and_nearheap::*;
use room_forms::*;
use store_spawn_and_viewer::*;
use watchdog_and_memory::*;

#[test]
fn m0_sandbox_suite() -> anyhow::Result<()> {
    let mut scenarios = kk_vm::test_support::ScenarioSuite::from_env()?;
    scenarios.run("prelude_basics", prelude_basics)?;
    scenarios.run("cjs_and_loop", cjs_and_loop)?;
    scenarios.run(
        "constant_overrides_reach_player_globals_impl",
        constant_overrides_reach_player_globals_impl,
    )?;
    scenarios.run("combat_range_constants_impl", combat_range_constants_impl)?;
    scenarios.run("move_gate_constants_impl", move_gate_constants_impl)?;
    scenarios.run("console_capture", console_capture)?;
    scenarios.run(
        "shared_targets_and_deprecation_lifetime",
        shared_targets_and_deprecation_lifetime,
    )?;
    scenarios.run("stub_game_projection", stub_game_projection)?;
    scenarios.run(
        "intent_channel_prelude_forms_impl",
        intent_channel_prelude_forms_impl,
    )?;
    scenarios.run("ext_intent_primitive_impl", ext_intent_primitive_impl)?;
    scenarios.run("mod_player_api_surface_impl", mod_player_api_surface_impl)?;
    scenarios.run(
        "mod_player_api_record_face_impl",
        mod_player_api_record_face_impl,
    )?;
    scenarios.run(
        "mod_player_api_module_gates_impl",
        mod_player_api_module_gates_impl,
    )?;
    scenarios.run(
        "mod_player_api_global_surface_impl",
        mod_player_api_global_surface_impl,
    )?;
    scenarios.run("hostile_probes_impl", hostile_probes_impl)?;
    scenarios.run(
        "move_misc_family_prelude_forms_impl",
        move_misc_family_prelude_forms_impl,
    )?;
    scenarios.run(
        "pathfinder_search_family_impl",
        pathfinder_search_family_impl,
    )?;
    scenarios.run(
        "move_to_never_paths_through_walls_impl",
        move_to_never_paths_through_walls_impl,
    )?;
    scenarios.run(
        "pathfinder_callback_family_impl",
        pathfinder_callback_family_impl,
    )?;
    scenarios.run(
        "pathfinder_matrix_mutation_impl",
        pathfinder_matrix_mutation_impl,
    )?;
    scenarios.run(
        "pathfinder_callback_budget_impl",
        pathfinder_callback_budget_impl,
    )?;
    scenarios.run(
        "pathfinder_kkpv_channel_equivalence_impl",
        pathfinder_kkpv_channel_equivalence_impl,
    )?;
    scenarios.run(
        "same_revision_world_switch_uses_current_terrain_impl",
        same_revision_world_switch_uses_current_terrain_impl,
    )?;
    scenarios.run("run_expr_channel", run_expr_channel)?;
    scenarios.run("wasm_binary_module", wasm_binary_module)?;
    scenarios.run(
        "watchdog_hard_terminate_and_reuse",
        watchdog_hard_terminate_and_reuse,
    )?;
    scenarios.run(
        "watchdog_late_expiry_does_not_kill_next_tick",
        watchdog_late_expiry_does_not_kill_next_tick,
    )?;
    scenarios.run(
        "memory_gate_terminates_and_reuses",
        memory_gate_terminates_and_reuses,
    )?;
    scenarios.run("skip_recharge_candidate_impl", skip_recharge_candidate_impl)?;
    scenarios.run("lifo_registry_drains", lifo_registry_drains)?;
    scenarios.run("memory_alias_setters", memory_alias_setters)?;
    scenarios.run(
        "roomposition_official_semantics",
        roomposition_official_semantics,
    )?;
    scenarios.run("roomposition_packedpos_forms", roomposition_packedpos_forms)?;
    scenarios.run("roomlook_family_forms", roomlook_family_forms)?;
    scenarios.run(
        "roomfind_exits_and_path_codec_forms",
        roomfind_exits_and_path_codec_forms,
    )?;
    scenarios.run(
        "terrain_look_and_console_semantics",
        terrain_look_and_console_semantics,
    )?;
    scenarios.run(
        "terrain_compact_protocol_form",
        terrain_compact_protocol_form,
    )?;
    scenarios.run(
        "writeback_watchdog_and_contract",
        writeback_watchdog_and_contract,
    )?;
    scenarios.run("stub_value_shapes", stub_value_shapes)?;
    scenarios.run(
        "official_string_and_error_shapes",
        official_string_and_error_shapes,
    )?;
    scenarios.run("run_loop_without_loop_silent", run_loop_without_loop_silent)?;
    scenarios.run(
        "writeback_rearmed_after_cancel",
        writeback_rearmed_after_cancel,
    )?;
    scenarios.run("store_class_semantics_impl", store_class_semantics_impl)?;
    scenarios.run("spawn_family_api_impl", spawn_family_api_impl)?;
    scenarios.run(
        "resource_family_prelude_matrix_impl",
        resource_family_prelude_matrix_impl,
    )?;
    scenarios.run("viewer_meta_semantics_impl", viewer_meta_semantics_impl)?;
    scenarios.run("claim_gate_gcl_behavior_impl", claim_gate_gcl_behavior_impl)?;
    scenarios.run(
        "redirect_upgrade_positive_impl",
        redirect_upgrade_positive_impl,
    )?;
    scenarios.run("destroy_room_intent_channel", destroy_room_intent_channel)?;
    scenarios.run("host_face_closed", host_face_closed)?;
    scenarios.run(
        "strict_frame_hides_host_functions",
        strict_frame_hides_host_functions,
    )?;
    scenarios.run(
        "nearheap_trigger_then_restore_invariant",
        nearheap_trigger_then_restore_invariant,
    )?;
    scenarios.run(
        "kkpv_channel_equivalence_impl",
        kkpv_channel_equivalence_impl,
    )?;
    scenarios.run("kkpv_ref_decode_impl", kkpv_ref_decode_impl)?;
    scenarios.run("kkpv_empty_bundle_impl", kkpv_empty_bundle_impl)?;
    scenarios.run(
        "kkpv_truncated_bundle_fails_impl",
        kkpv_truncated_bundle_fails_impl,
    )?;
    scenarios.run(
        "kkpv_shared_bundle_cross_thread_impl",
        kkpv_shared_bundle_cross_thread_impl,
    )?;
    // LIFO 零违规断言在**全部**场景之后——中段断言会让其后场景的违规漏网
    scenarios.run(
        "movement_codec_capture_and_cache_lifetime",
        movement_codec_capture_and_cache_lifetime,
    )?;
    scenarios.finish(58)?;
    assert_eq!(kk_runner::lifo_violations(), 0, "全流程 LIFO 纪律零违规");
    Ok(())
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-bot")
}

fn world() -> serde_json::Value {
    // 地形用全平原数组形态（宿主手搓夹具路径）：寻路/地形读取依赖每房地形。
    let plains: Vec<u8> = vec![0; 2500];
    json!({
        "time": 1,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "p1",
        "rooms": { "W1N1": { "name": "W1N1", "terrain": plains } },
        "objects": [
            { "id": "c1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "w1",
              "owner": "p1", "body": ["move", "work"], "store": { "energy": 50 } },
            { "id": "s1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "p1", "store": { "energy": 300 } },
            { "id": "k1", "type": "structure", "structureType": "controller", "room": "W1N1",
              "x": 10, "y": 10, "level": 3, "progress": 100, "progressTotal": 1000 },
            { "id": "src1", "type": "structure", "structureType": "source", "room": "W1N1",
              "x": 15, "y": 15, "energy": 2500 }
        ]
    })
}

fn load_sample_bot(sb: &mut Sandbox) -> anyhow::Result<()> {
    let table = ModuleTable::from_directory(&fixtures_dir())?;
    assert!(table.len() >= 4, "样例 bot 至少 4 个模块");
    sb.set_modules(table)
}

fn tick(sb: &mut Sandbox, w: &serde_json::Value) -> anyhow::Result<TickOutcome> {
    match sb.begin_tick(w)? {
        // 此辅助函数模拟即时提交的调用方：停跑时也提交回充。
        TickPlan::SkipPenalty => {
            sb.settle_bill();
            Ok(TickOutcome::Terminated)
        }
        TickPlan::Run { .. } => {
            let mut out = sb.run_loop()?;
            // 执行结束后立即提交费用；若写回被中断，同步返回 Terminated。
            let wb = sb.end_tick()?;
            sb.settle_bill();
            if wb.get("terminated").and_then(serde_json::Value::as_bool) == Some(true) {
                out = TickOutcome::Terminated;
            }
            Ok(out)
        }
    }
}

/// 全局面白名单：ECMA 内建 + 平铺常量（全大写下划线）+ 官方游戏类 + 环境注入物。
/// deno_core 升级新增的全局若不在其中，遮蔽复查测试即失败（人工复核后增删）。
fn is_allowed_global(name: &str) -> bool {
    const ECMA_BUILTINS: &[&str] = &[
        "globalThis",
        "Infinity",
        "NaN",
        "undefined",
        "eval",
        "isFinite",
        "isNaN",
        "parseFloat",
        "parseInt",
        "decodeURI",
        "decodeURIComponent",
        "encodeURI",
        "encodeURIComponent",
        "escape",
        "unescape",
        "Object",
        "Function",
        "Array",
        "String",
        "Number",
        "Boolean",
        "Symbol",
        "BigInt",
        "Error",
        "EvalError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "TypeError",
        "URIError",
        "AggregateError",
        "SuppressedError",
        "JSON",
        "Math",
        "Date",
        "RegExp",
        "ArrayBuffer",
        "SharedArrayBuffer",
        "DataView",
        "Map",
        "Set",
        "Iterator",
        "WeakMap",
        "WeakSet",
        "WeakRef",
        "FinalizationRegistry",
        "Promise",
        "Reflect",
        "Proxy",
        "Intl",
        "Temporal",
        "Atomics",
        "DisposableStack",
        "AsyncDisposableStack",
        "Uint8Array",
        "Int8Array",
        "Uint8ClampedArray",
        "Uint16Array",
        "Int16Array",
        "Uint32Array",
        "Int32Array",
        "BigUint64Array",
        "BigInt64Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "WebAssembly",
        "console",
        "global",
    ];
    const ENV_NAMES: &[&str] = &[
        // prelude 注入
        "_",
        "Buffer",
        "require",
        "Game",
        "Memory",
        "RawMemory",
        // deno_core 注入（V8 内建微任务调度，无内部面可达）
        "queueMicrotask",
        // 官方游戏类（GAME_CLASS_NAMES + 独立类 + 两个别名）
        "ConstructionSite",
        "Creep",
        "Deposit",
        "Flag",
        "Mineral",
        "Nuke",
        "OwnedStructure",
        "PowerCreep",
        "Resource",
        "Ruin",
        "Room",
        "RoomObject",
        "Source",
        "Store",
        "Structure",
        "StructureContainer",
        "StructureController",
        "StructureExtension",
        "StructureExtractor",
        "StructureFactory",
        "StructureInvaderCore",
        "StructureKeeperLair",
        "StructureLab",
        "StructureLink",
        "StructureNuker",
        "StructureObserver",
        "StructurePortal",
        "StructurePowerBank",
        "StructurePowerSpawn",
        "StructureRampart",
        "StructureRoad",
        "StructureSpawn",
        "StructureStorage",
        "StructureTerminal",
        "StructureTower",
        "StructureWall",
        "Tombstone",
        "Spawn",
        "Energy",
        "RoomPosition",
        "RoomVisual",
        "PathFinder",
    ];
    if ECMA_BUILTINS.contains(&name) || ENV_NAMES.contains(&name) {
        return true;
    }
    // 官方常量平铺（443 个，全大写 + 下划线，如 OK/FIND_CREEPS/BODYPARTS_ALL）。
    // 排除 __ 前缀：deno_core 的内部全局惯例是双下划线开头（如 __bootstrap），
    // 不能被全大写兜底放行
    name.len() > 1
        && !name.starts_with("__")
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}
