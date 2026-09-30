//! 嵌入式 API 的集成测试（M0）：createShard → place → runPlayer → tick → 读状态。

use serde_json::json;

use kk_runtime::EmbedEngine;

#[path = "embed_tests/dual_room_live_fire.rs"]
mod dual_room_live_fire;
#[path = "embed_tests/embed_channels_and_families.rs"]
mod embed_channels_and_families;
#[path = "embed_tests/embed_roundtrip_contract.rs"]
mod embed_roundtrip_contract;
#[path = "embed_tests/embed_strictness_and_partial_commit.rs"]
mod embed_strictness_and_partial_commit;

// 串行链不变量：顶层唯一 #[test]；子模块 impl 经 glob 进根作用域，链体逐字节不动
use dual_room_live_fire::*;
use embed_channels_and_families::*;
use embed_roundtrip_contract::*;
use embed_strictness_and_partial_commit::*;

#[test]
fn embed_engine_suite() -> anyhow::Result<()> {
    // 场景必须串行（真沙箱场景经 PlayerThreads 各占线程；进程级终局计数器
    // 断言要求不并行污染——单测试串行不变量沿用 sandbox_tests 惯例）
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
    assert_eq!(kk_runner::lifo_violations(), 0, "LIFO 纪律零违规");
    Ok(())
}
