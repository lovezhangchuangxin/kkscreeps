// 构造 pf.cc 所需的值对象，管理搜索实例，并将结果转换为 C ABI。
// 上游算法与平局规则在 pf.cc/pf.h；本文件的来源版本见 src/lib.rs。
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

// 每层 FFI 调用有自己的值存储和异常状态。嵌套调用结束后恢复外层，
// 内层错误已由 Rust 回调处理时，不得继续污染外层的 TryCatch。

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

// 缓存两份零初始化的实例；超过两层时由调用方临时分配新实例。
// reverse_room_table 初始须为 0，pf.cc 的 room_index_from_pos 将 0 视为未加载。
// 来源：上游 driver/native/src/main.cc 的 search。
// 池用指针化 thread_local：path_finder_t 约 2MB/实例，值形态的 TLS 数组
// 会让进程内所有线程（含从不搜索的 V8 平台工作线程）在线程启动时把整块
// TLS 记入物理内存；指针形态只有真正执行搜索的线程才付出分配。
screeps::path_finder_t* acquire_path_finder() {
	static thread_local std::unique_ptr<std::array<screeps::path_finder_t, 2>> pool;
	if (!pool) {
		pool = std::make_unique<std::array<screeps::path_finder_t, 2>>();
	}
	for (auto& ii : *pool) {
		if (!ii.is_in_use()) {
			return &ii;
		}
	}
	return nullptr;
}

} // namespace

// pf.h 声明的共享零矩阵；上游在 main.cc 中定义，这里提供同名外部链接符号。
uint8_t screeps::room_info_t::cost_matrix0[2500] = { 0 };

// 函数级 try 覆盖 arena、输入值构造和地形复制，不能只捕获算法调用。
extern "C" int kk_pf_load_terrain(uint8_t map_xx, uint8_t map_yy, const uint8_t* bits_625) try {
	if (bits_625 == nullptr) {
		return -1;
	}
	ArenaGuard guard;
	// pf.cc 的 load_terrain 接收房间信息数组，即使只载入一个房间也须包装成数组。
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

// 完整入口捕获所有 C++ 异常，防止分配失败穿过 Rust 的 extern "C" 调用。
extern "C" int kk_pf_search(kk_pf_world_pos_t origin, const kk_pf_goal_t* goals, size_t goals_len,
	kk_pf_room_callback_t callback, void* callback_user_data, const kk_pf_options_t* options,
	kk_pf_search_result_t* out_result) try {
	if (goals == nullptr && goals_len != 0) {
		return -1;
	}
	ArenaGuard guard;

	v8::Local<v8::Array> goals_js = Nan::New<v8::Array>(static_cast<int>(goals_len));
	for (size_t ii = 0; ii < goals_len; ++ii) {
		v8::Local<v8::Object> goal = Nan::New<v8::Object>();
		Nan::Set(goal, Nan::New("range").ToLocalChecked(), Nan::New(goals[ii].range));
		Nan::Set(goal, Nan::New("pos").ToLocalChecked(), make_pos_object(goals[ii].xx, goals[ii].yy));
		Nan::Set(goals_js, static_cast<uint32_t>(ii), goal);
	}

	// pf.cc 用 Undefined 关闭回调；有回调时由 compat/nan.h 的 Call 调用函数指针。
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
		// 搜索可能在异常发生时保留 is_in_use；避免将未清理的池实例用于后续搜索。
		// 输入构造和结果转换的异常由函数最外层 catch 一并拦截。
		return -1;
	}

	std::memset(out_result, 0, sizeof(*out_result));
	if (ret->IsUndefined()) {
		if (kk_compat::t_pending_js_exception) {
			// Undefined 既可表示到达目标，也可表示异常，须检查本层异常状态。
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
