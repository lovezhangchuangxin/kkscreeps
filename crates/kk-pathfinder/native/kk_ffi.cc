// kk-pathfinder 自有文件（非官方拷贝）：把官方 pf.cc 挂到 C ABI 上。
//
// 结论：本文件只做「数据搬运 + 结果分类」，不含任何寻路逻辑；算法与 tie-breaking
// 全部发生在未改动的官方 pf.cc 内。
//
// 对照证据：
// - path_finder_t 实例管理 ↔ 官方 main.cc:11-27（thread_local 双实例池 +
//   is_in_use 守卫 + 兜底堆分配；新实例零初始化由 C++ 静态零初始化/值初始化保证，
//   reverse_room_table 必须 initially-zero，pf.cc:23,470-472 依赖这一点）；
// - search 参数对象构造形态 ↔ driver/lib/path-finder.js:44-101（origin {xx,yy}、
//   goals [{range,pos:{xx,yy}}]、回调、九个标量选项）；
// - 结果分类 ↔ driver/lib/path-finder.js:96-102（undefined→空+incomplete:false、
//   -1→空+incomplete:true、对象→path/ops/cost/incomplete）。

#include "kk_ffi.h"

#include <array>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <limits>
#include <memory>
#include <new>
#include <utility>
#include <vector>

#include "pf.h" // 官方源码，原样编译（见同目录 pf.cc/pf.h）

namespace {

using v8::Value;

// ===== 值 arena（一次 FFI 调用的存活期） =====

class ArenaGuard {
public:
	ArenaGuard()
		: saved_(kk_compat::t_arena),
		  saved_exception_(kk_compat::t_pending_js_exception),
		  saved_message_(kk_compat::t_pending_js_error_message) {
		kk_compat::t_arena = &arena_;
		kk_compat::t_pending_js_exception = false;
		kk_compat::t_pending_js_error_message = nullptr;
	}
	~ArenaGuard() {
		kk_compat::t_arena = saved_;
		kk_compat::t_pending_js_exception = saved_exception_;
		kk_compat::t_pending_js_error_message = saved_message_;
	}

	ArenaGuard(const ArenaGuard&) = delete;
	ArenaGuard& operator=(const ArenaGuard&) = delete;

private:
	kk_compat::Arena arena_;
	kk_compat::Arena* saved_;
	bool saved_exception_;
	const char* saved_message_;
};

v8::Local<v8::Object> make_pos_object(uint32_t xx, uint32_t yy) {
	v8::Local<v8::Object> pos = Nan::New<v8::Object>();
	Nan::Set(pos, Nan::New("xx").ToLocalChecked(), Nan::New(xx));
	Nan::Set(pos, Nan::New("yy").ToLocalChecked(), Nan::New(yy));
	return pos;
}

// 官方 main.cc:11-27 的实例获取方式：thread_local 池 + is_in_use + 兜底。
// 递归 search（roomCallback 内再触发寻路）时官方同样靠双实例池支持两层深度。
screeps::path_finder_t* acquire_path_finder() {
	static thread_local std::array<screeps::path_finder_t, 2> pool;
	for (auto& ii : pool) {
		if (!ii.is_in_use()) {
			return &ii;
		}
	}
	return nullptr;
}

} // namespace

// pf.h:217 静态成员的定义（官方在 main.cc:27-29 于 namespace screeps 内给出，
// 此处等价提供——非匿名命名空间，保证外部链接符号与 pf.cc 引用一致）
uint8_t screeps::room_info_t::cost_matrix0[2500] = { 0 };

extern "C" int kk_pf_load_terrain(uint8_t map_xx, uint8_t map_yy, const uint8_t* bits_625) try {
	if (bits_625 == nullptr) {
		return -1;
	}
	ArenaGuard guard;
	// loadTerrain([{ room: {xx,yy}, bits: Uint8Array(625) }]），形态对照
	// driver/lib/path-finder.js:12-30（init）与 pf.cc:605-613（load_terrain）。
	v8::Local<v8::Array> rooms = Nan::New<v8::Array>(1);
	v8::Local<v8::Object> info = Nan::New<v8::Object>();
	Nan::Set(info, Nan::New("room").ToLocalChecked(), make_pos_object(map_xx, map_yy));
	Value* bits = kk_compat::t_arena->alloc();
	bits->type = v8::Value::TARRAY;
	bits->tarray_data = bits_625;
	bits->tarray_len = 625;
	Nan::Set(info, Nan::New("bits").ToLocalChecked(), v8::Local<v8::Value>(bits));
	Nan::Set(rooms, 0u, info);
	screeps::path_finder_t::load_terrain(rooms);
	return 0;
} catch (...) {
	return -1;
}

extern "C" int kk_pf_search(kk_pf_world_pos_t origin, const kk_pf_goal_t* goals, size_t goals_len,
	kk_pf_room_callback_t callback, void* callback_user_data, const kk_pf_options_t* options,
	kk_pf_search_result_t* out_result) try {
	if (goals == nullptr && goals_len != 0) {
		return -1;
	}
	ArenaGuard guard;

	// goals：[{ range, pos: {xx,yy} }]（pf.h:238-247 goal_t 从该形态构造）
	v8::Local<v8::Array> goals_js = Nan::New<v8::Array>(static_cast<int>(goals_len));
	for (size_t ii = 0; ii < goals_len; ++ii) {
		v8::Local<v8::Object> goal = Nan::New<v8::Object>();
		Nan::Set(goal, Nan::New("range").ToLocalChecked(), Nan::New(goals[ii].range));
		Nan::Set(goal, Nan::New("pos").ToLocalChecked(), make_pos_object(goals[ii].xx, goals[ii].yy));
		Nan::Set(goals_js, static_cast<uint32_t>(ii), goal);
	}

	// 回调句柄：nullptr → undefined（pf.cc:488-492 据此禁用回调通道）；
	// 否则注入 C 函数指针 + user_data（Nan::Call 分派，见 compat/nan.h；返回码
	// 0/1/2/3 的语义见 kk_ffi.h 的 kk_pf_room_callback_t 注释——3 = 回调异常，
	// 经 compat 层汇入官方 js_error 处理链 pf.cc:43-46 → :563-567）。
	v8::Local<v8::Function> room_callback;
	if (callback != nullptr) {
		Value* fn = kk_compat::t_arena->alloc();
		fn->type = v8::Value::FUNCTION;
		fn->fn_ptr = callback;
		fn->fn_user = callback_user_data;
		room_callback = v8::Local<v8::Function>(fn);
	} else {
		room_callback = v8::Local<v8::Function>(Nan::Undefined().v);
	}

	screeps::path_finder_t* pf = acquire_path_finder();
	std::unique_ptr<screeps::path_finder_t> holder;
	if (pf == nullptr) {
		holder = std::make_unique<screeps::path_finder_t>();
		pf = holder.get();
	}

	v8::Local<v8::Value> ret;
	try {
		ret = pf->search(make_pos_object(origin.xx, origin.yy), goals_js, room_callback,
			options->plain_cost, options->swamp_cost, static_cast<uint8_t>(options->max_rooms),
			options->max_ops, options->max_cost, options->flee != 0, options->heuristic_weight);
	} catch (...) {
		// pf.cc 仅两条异常路径：js_error（已在其内部捕获，pf.cc:563）与
		// index_from_pos / heap 满的 runtime_error（pf.cc:70,303——正常输入下
		// 不可达，所有 push 均有 obstacle 前置过滤）。兜底防异常穿越 C ABI。
		return -1;
	}

	std::memset(out_result, 0, sizeof(*out_result));
	if (ret->IsUndefined()) {
		if (kk_compat::t_pending_js_exception) {
			// js_error 路径（官方上抛：pf.cc:44 ReThrow / :33 ThrowError）——
			// 与同格搜索的合法 undefined（pf.cc:508-510）区分
			out_result->status = 3;
			out_result->js_error_message = kk_compat::t_pending_js_error_message;
		} else {
			out_result->status = 2;
		}
		return 0;
	}
	if (ret->type == v8::Value::NUMBER && static_cast<int64_t>(ret->num) == -1) {
		out_result->status = 1;
		return 0;
	}

	// { path: [[xx,yy]...], ops, cost, incomplete }（pf.cc:595-599）
	Value* path = ret->get_prop("path");
	Value* ops = ret->get_prop("ops");
	Value* cost = ret->get_prop("cost");
	Value* incomplete = ret->get_prop("incomplete");
	if (path == nullptr || path->type != v8::Value::ARRAY || ops == nullptr || cost == nullptr ||
		incomplete == nullptr) {
		return -1;
	}
	std::vector<uint32_t> flat;
	flat.reserve(path->elems.size() * 2);
	for (const Value* step : path->elems) {
		if (step == nullptr || step->type != v8::Value::ARRAY || step->elems.size() < 2 ||
			step->elems[0] == nullptr || step->elems[1] == nullptr) {
			return -1;
		}
		flat.push_back(static_cast<uint32_t>(step->elems[0]->num));
		flat.push_back(static_cast<uint32_t>(step->elems[1]->num));
	}
	out_result->ops = static_cast<uint32_t>(ops->num);
	out_result->cost = static_cast<uint32_t>(cost->num);
	out_result->incomplete = incomplete->b ? 1u : 0u;
	out_result->path_len = static_cast<uint32_t>(flat.size() / 2);
	size_t alloc_count = flat.empty() ? 1 : flat.size();
	out_result->path =
		static_cast<uint32_t*>(std::malloc(sizeof(uint32_t) * alloc_count));
	if (out_result->path == nullptr) {
		return -1;
	}
	if (!flat.empty()) {
		std::memcpy(out_result->path, flat.data(), sizeof(uint32_t) * flat.size());
	}
	return 0;
} catch (...) {
	return -1;
}

extern "C" void kk_pf_free_result(kk_pf_search_result_t* result) {
	if (result != nullptr && result->path != nullptr) {
		std::free(result->path);
		result->path = nullptr;
	}
}
