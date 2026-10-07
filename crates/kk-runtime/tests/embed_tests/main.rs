//! 嵌入式 API 的集成测试：创建分片、摆放对象、运行玩家、推进世界并读取结果。
//!
//! 所有场景由一个测试顺序调用，避免并行场景干扰进程级沙箱销毁计数。

use serde_json::json;

use kk_runtime::EmbedEngine;

mod channels;
mod creation;
mod creep_actions;
mod dual_room_live_fire;
mod economy_gcl_family;
mod embed_roundtrip_contract;
mod embed_strictness_and_partial_commit;
mod execution_boundary;
mod ext_intent_counter;
mod game_api;
mod input_log_family;
mod map;
mod mod_player_api;
mod mod_settle;
mod replay_family;
mod sim_setup_family;
mod stage_recovery_family;
mod stop_run_regression;
mod structure_actions;
mod structure_readfaces;
mod terminal_market;

use channels::*;
use creation::*;
use creep_actions::*;
use dual_room_live_fire::*;
use embed_roundtrip_contract::*;
use embed_strictness_and_partial_commit::*;
use game_api::*;
use input_log_family::*;
use map::*;
use replay_family::*;
use stage_recovery_family::*;
use stop_run_regression::*;
use structure_actions::*;
use structure_readfaces::*;
use terminal_market::*;

#[test]
fn embed_engine_suite() -> anyhow::Result<()> {
    // 沙箱销毁统计是进程级的；这些场景串行运行，避免统计被并行场景干扰。
    embed_engine_roundtrip_impl()?;
    economy_gcl_family::economy_gcl_e2e_impl()?;
    sim_setup_family::sim_setup_preserves_tick_and_failure_candidates_impl()?;
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
    kkpv_public_bundle_pin_hostile_impl()?;
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
    dual_room_live_fire::dual_inter_room_transition_impl()?;
    dual_execution_order_invariance_impl()?;
    dual_multi_tick_conservation_impl()?;
    dual_lifecycle_spawn_and_construction_impl()?;
    dual_room_failure_recovery_impl()?;
    dual_recovery_variants_impl()?;
    stop_run_commit_blind_impl()?;
    stop_run_failure_and_discard_impl()?;
    stop_run_floor_window_impl()?;
    create_flag_and_site_e2e_impl()?;
    creation_gates_and_helpers_e2e_impl()?;
    storage_terminal_backfill_e2e_impl()?;
    ranged_attack_e2e_impl()?;
    saying_visibility_e2e_impl()?;
    notify_when_attacked_creep_e2e_impl()?;
    map_family_e2e_impl()?;
    map_multihop_e2e_impl()?;
    notify_and_cpu_e2e_impl()?;
    structure_readfaces_e2e_impl()?;
    structure_defaults_probe_impl()?;
    structure_actions_e2e_impl()?;
    b02_link_constants_injected_impl()?;
    terminal_send_e2e_impl()?;
    market_book_e2e_impl()?;
    segments_and_visuals_e2e_impl()?;
    stage_recovery_family_impl()?;
    execution_boundary::multi_player_writeback_retry_impl()?;
    execution_failure_blocks_redispatch_impl()?;
    lost_worker_rebuild_impl()?;
    bounded_parallel_players_impl()?;
    stage_recovery_escape_route_impl()?;
    stage_hook_deferral_impl()?;
    mod_settle::mod_settle_state_accumulates_impl()?;
    mod_settle::mod_settle_failure_rolls_back_state_impl()?;
    mod_settle::mod_settle_replace_and_wrap_gate_impl()?;
    mod_settle::mod_settle_registration_failure_impl()?;
    ext_intent_counter::counter_full_chain_impl()?;
    mod_player_api::player_chain_full_impl()?;
    mod_player_api::dual_channel_parity_impl()?;
    mod_player_api::field_visibility_cut_impl()?;
    mod_player_api::install_gates_impl()?;
    mod_player_api::server_only_type_impl()?;
    mod_player_api::float_field_dual_channel_impl()?;
    ext_intent_counter::counter_gates_impl()?;
    ext_intent_counter::counter_failure_rolls_back_impl()?;
    ext_intent_counter::counter_creation_gates_impl()?;
    ext_intent_counter::counter_config_and_projection_impl()?;
    ext_intent_counter::counter_creation_and_round_gates_impl()?;
    ext_intent_counter::counter_link_only_shard_skips_primitive_impl()?;
    ext_intent_counter::counter_suppression_impl()?;
    ext_intent_counter::counter_suppression_freeze_gates_impl()?;
    three_sources_round_trip_impl()?;
    failed_round_and_retry_semantics_impl()?;
    log_io_failure_commits_round_and_detaches_impl()?;
    tamper_matrix_impl()?;
    torn_tail_byte_forms_impl()?;
    three_sources_replay_matches_live_impl()?;
    dice_replay_matches_live_impl()?;
    pairing_matrix_impl()?;
    identity_and_tick_gates_impl()?;
    rebuild_writeback_null_and_some_impl()?;
    settle_replayed_round_contract_impl()?;

    cursor_and_header_pairing_impl()?;
    resume_and_lifecycle_impl()?;
    assert_eq!(kk_runner::lifo_violations(), 0, "LIFO 纪律零违规");
    Ok(())
}
