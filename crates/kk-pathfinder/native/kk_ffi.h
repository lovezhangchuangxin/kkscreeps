// kk-pathfinder 自有文件（非官方拷贝）：官方 pf.cc 的 C ABI 包装声明。
// Rust 绑定见 src/ffi.rs；参数形态对照官方 JS 调用面：
// - search 实参 ↔ driver/lib/path-finder.js:134（mod.search(origin, goals, cb,
//   plainCost, swampCost, maxRooms, maxOps, maxCost, flee, heuristicWeight)）；
// - origin/goals 的世界坐标形态 ↔ driver/lib/path-finder.js:34-44（toWorldPosition）；
// - 地形加载 ↔ driver/lib/path-finder.js:12-30 + pf.cc:605-613（loadTerrain）。
#ifndef KK_PATHFINDER_FFI_H_
#define KK_PATHFINDER_FFI_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// 世界连续平面坐标（pf.h:59-81 world_position_t；JS 侧换算见 path-finder.js:34-44）
typedef struct kk_pf_world_pos {
	uint32_t xx;
	uint32_t yy;
} kk_pf_world_pos_t;

// goal：世界坐标 + range（pf.h:238-247 goal_t；range 语义 = Chebyshev 距离域）
typedef struct kk_pf_goal {
	uint32_t xx;
	uint32_t yy;
	uint32_t range;
} kk_pf_goal_t;

// 选项（钳制在 Rust 侧完成后传入；对照 driver/lib/path-finder.js:64-70）
typedef struct kk_pf_options {
	uint32_t plain_cost;
	uint32_t swamp_cost;
	uint32_t max_ops;
	uint32_t max_cost;
	uint32_t max_rooms; // 1..=64（C++ 侧为 uint8_t，pf.cc:462）
	uint32_t flee;      // 0/1
	double heuristic_weight;
} kk_pf_options_t;

// roomCallback 的 C 形态：官方语义 = JS 回调返回 undefined / false / Uint8Array
// （pf.cc:37-59、driver/lib/path-finder.js:120-129）。矩阵长度透传：官方 JS bridge
// 无条件交出 `ret._bits`，长度过滤（== 2500）在 pf.cc:54-57 完成——错误长度静默按
//「无矩阵」处理。因此本契约不预设 2500，回调须把真实长度写入 *out_len，由官方
// pf.cc 的长度检查裁决（与官方控制流逐点一致）。
// 返回：0 = undefined（该房间无 CostMatrix）；1 = false（封锁该房间）；
//       2 = 通过 *out_matrix/*out_len 提供矩阵（布局 [xx*50 + yy]，CostMatrix._bits）；
//       3 = 回调抛异常（官方 roomCallback throw 的等价物：compat 层置挂起异常，
//            pf.cc:43-46 捕获 → js_error → pf.cc:563-567 → search 返回 undefined）。
// *out_matrix 指向的存储必须存活到本次 search 返回（pf.cc 直接引用不拷贝）。
typedef int (*kk_pf_room_callback_t)(void* user_data, uint8_t xx, uint8_t yy,
	const uint8_t** out_matrix, size_t* out_len);

// search 结果。status：0 = 正常对象结果；1 = native 返回 -1（初始房间不可达，
// pf.cc:515-519）；2 = native 返回 undefined（同格搜索/JS 异常，pf.cc:508-510,563-567）。
// path 为 malloc 分配的 [xx,yy] 对数组，goal→origin 序（native 原序，pf.cc:574-594；
// JS 侧才做 reverse，driver/lib/path-finder.js:101），用 kk_pf_free_result 释放。
typedef struct kk_pf_search_result {
	int32_t status;
	uint32_t ops;
	uint32_t cost;
	uint32_t incomplete;
	uint32_t path_len;
	uint32_t* path;
} kk_pf_search_result_t;

// 返回 0 = 成功（细节看 out_result）；非 0 = C++ 异常逃逸（理论不可达）或内存不足。
int kk_pf_search(kk_pf_world_pos_t origin, const kk_pf_goal_t* goals, size_t goals_len,
	kk_pf_room_callback_t callback, void* callback_user_data, const kk_pf_options_t* options,
	kk_pf_search_result_t* out_result);

void kk_pf_free_result(kk_pf_search_result_t* result);

// 载入一个房间的地形（625 字节 packed bits；格式与 driver/lib/path-finder.js:20-27
// 一致）。bits 仅在本调用期内被读取（pf.cc:606-612 内部 memcpy 走自有分配）。
// 可多次调用（不同房间）；对同一房间重复调用会用新数据覆盖（与官方多次
// loadTerrain 行为一致，旧数据不回收——官方同样如此，见 pf.cc:605-613）。
int kk_pf_load_terrain(uint8_t map_xx, uint8_t map_yy, const uint8_t* bits_625);

#ifdef __cplusplus
}
#endif

#endif // KK_PATHFINDER_FFI_H_
