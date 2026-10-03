//! 嵌入式 API 的集成测试：创建分片、摆放对象、运行玩家、推进世界并读取结果。
//!
//! 所有场景由一个测试顺序调用，避免并行场景干扰进程级沙箱销毁计数。

use serde_json::json;

use kk_runtime::EmbedEngine;

#[path = "embed_tests/c3_counter_chain_family.rs"]
mod c3_counter_chain;
#[path = "embed_tests/c4_player_api_family.rs"]
mod c4_player_api;
#[path = "embed_tests/c_mod_settle_family.rs"]
mod c_mod_settle;
#[path = "embed_tests/dual_room_live_fire.rs"]
mod dual_room_live_fire;
#[path = "embed_tests/embed_channels_and_families.rs"]
mod embed_channels_and_families;
#[path = "embed_tests/embed_roundtrip_contract.rs"]
mod embed_roundtrip_contract;
#[path = "embed_tests/embed_strictness_and_partial_commit.rs"]
mod embed_strictness_and_partial_commit;
#[path = "embed_tests/m2b2_terminal_market_family.rs"]
mod m2b2_terminal_market_family;
#[path = "embed_tests/m2b_closing_family.rs"]
mod m2b_closing_family;
#[path = "embed_tests/m2b_creation_family.rs"]
mod m2b_creation_family;
#[path = "embed_tests/m2b_creep_tails.rs"]
mod m2b_creep_tails;
#[path = "embed_tests/m2b_map_notify_cpu_family.rs"]
mod m2b_map_notify_cpu_family;
#[path = "embed_tests/m2b_structure_actions_family.rs"]
mod m2b_structure_actions_family;
#[path = "embed_tests/m2b_structure_readfaces_family.rs"]
mod m2b_structure_readfaces_family;
#[path = "embed_tests/stage_recovery_family.rs"]
mod stage_recovery_family;
#[path = "embed_tests/stop_run_regression.rs"]
mod stop_run_regression;

use dual_room_live_fire::*;
use embed_channels_and_families::*;
use embed_roundtrip_contract::*;
use embed_strictness_and_partial_commit::*;
use m2b_closing_family::*;
use m2b_creation_family::*;
use m2b_creep_tails::*;
use m2b_map_notify_cpu_family::*;
use m2b_structure_actions_family::*;
use m2b_structure_readfaces_family::*;
use m2b2_terminal_market_family::*;
use stage_recovery_family::*;
use stop_run_regression::*;

#[test]
fn embed_engine_suite() -> anyhow::Result<()> {
    // 沙箱销毁统计是进程级的；这些场景串行运行，避免统计被并行场景干扰。
    embed_engine_roundtrip_impl()?;
    embed_run_player_contract_impl()?;
    embed_create_shard_validation_impl()?;
    embed_shard_input_strictness_impl()?;
    b05_partial_commit_inject_failure_impl()?;
    b05_partial_commit_settle_failure_impl()?;
    b06_rejected_retry_preserves_pending_impl()?;
    b06_retry_reuses_recorded_tick_input_impl()?;
    embed_real_settlement_impl()?;
    embed_run_players_shared_snapshot_impl()?;
    embed_snapshot_contract_impl()?;
    embed_intent_channel_impl()?;
    kkpv_channel_e2e_equivalence_impl()?;
    kkpv_injection_size_sample_impl()?;
    kkpv_multi_player_concurrency_impl()?;
    embed_per_viewer_and_caps_impl()?;
    embed_spawn_family_chain_impl()?;
    embed_resource_family_impl()?;
    embed_move_misc_family_impl()?;
    embed_mineral_direct_channel_impl()?;
    terrain_compact_protocol_e2e_impl()?;
    embed_gpt_review_clearance_impl()?;
    a5_memory_module_forms_impl()?;
    a6_isactive_hits_family_impl()?;
    dual_a_one_player_two_rooms_impl()?;
    dual_b_two_players_one_room_impl()?;
    dual_c_unrelated_room_independent_impl()?;
    dual_execution_order_invariance_impl()?;
    dual_multi_tick_conservation_impl()?;
    dual_lifecycle_spawn_and_construction_impl()?;
    dual_room_failure_recovery_impl()?;
    dual_b06_variants_impl()?;
    stop_run_commit_blind_impl()?;
    stop_run_failure_and_discard_impl()?;
    stop_run_floor_window_impl()?;
    m2b_create_flag_and_site_e2e_impl()?;
    m2b_creation_gates_and_helpers_e2e_impl()?;
    m2b_storage_terminal_backfill_e2e_impl()?;
    m2b_ranged_attack_e2e_impl()?;
    m2b_saying_visibility_e2e_impl()?;
    m2b_notify_when_attacked_creep_e2e_impl()?;
    m2b_map_family_e2e_impl()?;
    m2b_map_multihop_e2e_impl()?;
    m2b_notify_and_cpu_e2e_impl()?;
    m2b_structure_readfaces_e2e_impl()?;
    m2b_structure_defaults_probe_impl()?;
    m2b_structure_actions_e2e_impl()?;
    b02_link_constants_injected_impl()?;
    m2b2_terminal_send_e2e_impl()?;
    m2b2_market_book_e2e_impl()?;
    m2b_closing_e2e_impl()?;
    stage_recovery_family_impl()?;
    execution_failure_blocks_redispatch_impl()?;
    lost_worker_rebuild_impl()?;
    bounded_parallel_players_impl()?;
    stage_recovery_escape_route_impl()?;
    stage_hook_deferral_impl()?;
    c_mod_settle::c_mod_settle_state_accumulates_impl()?;
    c_mod_settle::c_mod_settle_failure_rolls_back_state_impl()?;
    c_mod_settle::c_mod_settle_replace_and_wrap_gate_impl()?;
    c_mod_settle::c_mod_settle_registration_failure_impl()?;
    c3_counter_chain::c3_counter_full_chain_impl()?;
    c4_player_api::c4_player_chain_full_impl()?;
    c4_player_api::c4_dual_channel_parity_impl()?;
    c4_player_api::c4_field_visibility_cut_impl()?;
    c4_player_api::c4_install_gates_impl()?;
    c4_player_api::c4_server_only_type_impl()?;
    c4_player_api::c4_float_field_dual_channel_impl()?;
    c3_counter_chain::c3_counter_gates_impl()?;
    c3_counter_chain::c3_counter_failure_rolls_back_impl()?;
    c3_counter_chain::c3_counter_creation_gates_impl()?;
    c3_counter_chain::c3_counter_config_and_projection_impl()?;
    c3_counter_chain::c3_counter_creation_and_round_gates_impl()?;
    c3_counter_chain::c3_counter_link_only_shard_skips_primitive_impl()?;
    assert_eq!(kk_runner::lifo_violations(), 0, "LIFO 纪律零违规");
    Ok(())
}
