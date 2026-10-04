// Screeps 寻路器的 C ABI；与 src/ffi.rs 的 repr(C) 结构体保持字段顺序、宽度一致。
// 来源版本见 src/lib.rs；算法保留在 pf.cc/pf.h，本接口负责数据转换和状态分类。
// 入口自身不加地形锁：调用方须让载入与所有搜索互斥；多个搜索可并发或嵌套。
#ifndef KK_PATHFINDER_FFI_H_
#define KK_PATHFINDER_FFI_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// 连续世界格子坐标：房内坐标 + 地图房间坐标 * 50。
typedef struct kk_pf_world_pos {
	uint32_t xx;
	uint32_t yy;
} kk_pf_world_pos_t;

// range 使用切比雪夫距离，普通搜索接近任一目标，flee 搜索离开全部目标范围。
typedef struct kk_pf_goal {
	uint32_t xx;
	uint32_t yy;
	uint32_t range;
} kk_pf_goal_t;

// 数值范围由 Rust 调用层调整，C ABI 不重新检查；尤其 max_rooms 必须在 1..=64。
typedef struct kk_pf_options {
	uint32_t plain_cost;
	uint32_t swamp_cost;
	uint32_t max_ops;
	uint32_t max_cost;
	uint32_t max_rooms; // 传给 C++ 时转为 uint8_t
	uint32_t flee;      // 0/1
	double heuristic_weight;
} kk_pf_options_t;

// 同步回调，参数是地图房间坐标。返回码：0=只用地形，1=封锁房间，
// 2=通过 out_matrix/out_len 借用矩阵，3=回调异常。
// 返回 2 时必须写入真实长度，只有 2500 字节的矩阵会由 pf.cc 接受。
// C++ 只读、不复制，缓冲须存活到整次 search 返回；user_data 同样须持续有效。
// 宿主可在同步 room 回调内（算法暂停）修改此前矩阵；其他时刻不得修改。
// 回调不能让异常越过 C ABI；Rust trampoline 负责捕获可展开的 panic。
// 来源：pf.cc 的 room_index_from_pos，以及 compat/nan.h 的 Call。
typedef int (*kk_pf_room_callback_t)(void* user_data, uint8_t xx, uint8_t yy,
	const uint8_t** out_matrix, size_t* out_len);

// 搜索主循环的取消检查；非零表示取消。本函数不得抛错或越过 ABI 展开。
typedef int (*kk_pf_cancel_callback_t)(void* user_data);

// status：0=搜索结果，1=初始房间被封锁，2=已在目标范围内的空结果，
// 3=搜索错误，4=取消。状态 1 的 Rust 结果 incomplete=true，状态 2 为 false；
// 状态 3/4 转为 Err；取消不能冒充成功的空路径。
// js_error_message 为可选 NUL 结尾消息，调用方应立即复制；回调异常通常为 null。
// path 是 malloc 分配的 xx/yy 对，从结果终点向起点排列，不包含起点。
// 即使 path_len=0 也可能分配路径内存，成功调用后须调用 kk_pf_free_result。
// 来源：pf.cc 的 search；结果顺序转换见 driver/lib/path-finder.js 的 search。
typedef struct kk_pf_search_result {
	int32_t status;
	const char* js_error_message;
	uint32_t ops;
	uint32_t cost;
	uint32_t incomplete;
	uint32_t path_len;
	uint32_t* path;
} kk_pf_search_result_t;

// options/out_result 必须有效，非空 goals 须指向 goals_len 个元素。
// 返回 0 时 out_result 有效（包含 status 3 的搜索错误）；非零表示包装层失败，
// 包括构造输入、运行算法或转换结果期间的 C++ 异常，此时应丢弃 out_result。
int kk_pf_search(kk_pf_world_pos_t origin, const kk_pf_goal_t* goals, size_t goals_len,
	kk_pf_room_callback_t callback, void* callback_user_data,
	kk_pf_cancel_callback_t cancel, const kk_pf_options_t* options,
	kk_pf_search_result_t* out_result);

// 释放成功调用的路径分配并将指针清空；允许 result 或 path 为 null。
void kk_pf_free_result(kk_pf_search_result_t* result);

// bits_625 指向 625 个字节，按列顺序每格两位；调用期间有效即可，pf.cc 会复制。
// 同一房间再次载入会覆盖表项，但上游不回收旧缓冲。返回 0 表示载入成功，
// 非零表示空指针或 C++ 异常。载入期间不能有其他载入或搜索。
int kk_pf_load_terrain(uint8_t map_xx, uint8_t map_yy, const uint8_t* bits_625);

#ifdef __cplusplus
}
#endif

#endif // KK_PATHFINDER_FFI_H_
