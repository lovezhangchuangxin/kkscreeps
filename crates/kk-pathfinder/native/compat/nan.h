// kk-pathfinder 自有文件（非官方拷贝）：官方 pf.cc/pf.h 的最小 Nan/v8 兼容层。
//
// 结论：官方寻路算法本体（pf.cc 的 A* / JPS / 堆序 / tie-breaking）不依赖 JS 引擎，
// 只依赖 v8 的四个原语——值表示、属性读写、数值转换、回调调用。本头文件用
// 「线程局部 arena 中的值节点」实现这四个原语，使官方源码零改动编译，路径结果
// （含堆内平局裁决）与官方逐点一致。
//
// 证据链：
// - pf.cc/pf.h 引用的 v8/Nan 符号清单：Local<Value/Object/Array/Function/String/Boolean>、
//   MaybeLocal/Maybe、Nan::{To,Get,Set,New,Call,Undefined,ThrowError,TryCatch,
//   TypedArrayContents}、v8::Isolate::GetCurrent()->IsExecutionTerminating()（全部出处
//   见各实现处注释）；这些符号在算法路径上只做数据搬运，不影响搜索顺序。
// - 搜索顺序（决定路径与 tie-breaking）完全由 pf.cc 自身的 heap_t::pop/bubble_up
//   （pf.h:273-330）与方向枚举（pf.h:69、pf.cc:152-153）决定，本兼容层不参与。
//
// 值模型与生命周期（unsafe 边界设计说明）：
// - v8::Value = arena（std::deque）分配的带标签节点；arena 由 FFI 入口
//   （kk_ffi.cc 的 ArenaGuard）安装到 thread_local，一次 FFI 调用结束时销毁。
// - v8::Local<T> 是非拥有句柄（裸指针视图），节点存活期覆盖 Local 的使用期——
//   与官方 HandleScope 语义等价：pf.cc 中所有 Local 均不逃逸出单次
//   search/load_terrain 调用（room_data_handles 也只在该调用栈内存活，pf.cc:486-487）。
// - Nan::Call 不进入 JS 引擎：Function 节点持有 C 函数指针 + user_data（由
//   kk_ffi.cc 注入），对应官方 roomCallback 通道（pf.cc:37-59）。
// - v8::Isolate::IsExecutionTerminating 恒为 false：官方用它响应 isolate 硬终止，
//   本环境无 JS 引擎可终止；搜索上界由 max_ops 保证（与官方一致，pf.cc:499,526）。

#ifndef KK_PATHFINDER_COMPAT_NAN_H_
#define KK_PATHFINDER_COMPAT_NAN_H_

#include <cstdint>
#include <cstring>
#include <deque>
#include <string>
#include <utility>
#include <vector>

namespace v8 {

// 带标签值节点。类型层次（String/Object/Array/...）仅用于 Local<T> 的静态转换，
// 运行时行为全部由 type 决定——与 v8「一切皆 Value」一致。
class Value {
public:
	enum type_t {
		UNDEFINED,
		BOOLEAN,
		NUMBER,
		STRING,
		TARRAY,   // Uint8Array
		OBJECT,
		ARRAY,
		FUNCTION,
	};

	type_t type = UNDEFINED;
	bool b = false;
	double num = 0;
	std::string str;
	const uint8_t* tarray_data = nullptr; // 指向调用方缓冲（CostMatrix/地形 bits）
	size_t tarray_len = 0;
	std::vector<std::pair<std::string, Value*>> props; // OBJECT 属性
	std::vector<Value*> elems;                          // ARRAY 元素（下标 Set 可增长）
	int (*fn_ptr)(void*, uint8_t, uint8_t, const uint8_t**) = nullptr; // FUNCTION
	void* fn_user = nullptr;

	bool IsUndefined() const { return type == UNDEFINED; }
	bool IsBoolean() const { return type == BOOLEAN; }
	bool IsFalse() const { return type == BOOLEAN && !b; }
	// v8::Array::Length（pf.cc:480,607 经 ->Length() 使用）
	uint32_t Length() const { return static_cast<uint32_t>(elems.size()); }

	Value* get_prop(const char* key) const {
		for (const auto& prop : props) {
			if (prop.first == key) {
				return prop.second;
			}
		}
		return nullptr;
	}
};

class String : public Value {};
class Boolean : public Value {};
class Object : public Value {};
class Array : public Object {};
class Function : public Object {};

class Isolate {
public:
	static Isolate* GetCurrent() {
		static Isolate instance;
		return &instance;
	}
	bool IsExecutionTerminating() const { return false; }
};

// 非拥有句柄。S* 可转换 T* 时允许 Local<S> → Local<T>（v8 的层次上转）。
template <class T>
class Local {
public:
	Value* v = nullptr;

	Local() = default;
	explicit Local(Value* v) : v(v) {}

	template <class S, class = std::enable_if_t<std::is_convertible_v<S*, T*>>>
	Local(const Local<S>& other) : v(other.v) {}

	T* operator->() const { return static_cast<T*>(v); }
	T& operator*() const { return *static_cast<T*>(v); }

	// v8::Local<T>::Cast——官方 pf.cc:42 使用 Local<Object>::Cast(Undefined())
	template <class S>
	static Local<T> Cast(const Local<S>& other) {
		Local<T> ret;
		ret.v = other.v;
		return ret;
	}
};

template <class T>
class MaybeLocal {
public:
	Local<T> val;
	bool present = false;

	MaybeLocal() = default;

	template <class S, class = std::enable_if_t<std::is_convertible_v<S*, T*>>>
	MaybeLocal(const Local<S>& v) : val(v), present(true) {}

	bool IsEmpty() const { return !present; }
	Local<T> ToLocalChecked() const { return val; }
};

template <class T>
class Maybe {
public:
	T val{};
	bool present = false;

	Maybe() = default;
	explicit Maybe(T v) : val(v), present(true) {}

	bool IsNothing() const { return !present; }
	T FromJust() const { return val; }
};

} // namespace v8

namespace kk_compat {

// 一次 FFI 调用存活期的值 arena。deque 保证节点地址稳定。
struct Arena {
	std::deque<v8::Value> nodes;
	v8::Value* alloc() {
		nodes.emplace_back();
		return &nodes.back();
	}
};

inline thread_local Arena* t_arena = nullptr;
// Nan::ThrowError 的挂起标志（pf.cc:33-34 唯一抛出点，随后立刻 throw js_error
// 被 pf.cc:563 捕获；TryCatch 理论上永远观察不到，保留标志仅为语义完整）。
inline thread_local bool t_pending_js_exception = false;

} // namespace kk_compat

namespace Nan {

// 官方 Nan 把 MaybeLocal/Maybe 暴露在 Nan:: 命名空间（pf.cc:42 使用
// `Nan::MaybeLocal<v8::Value>`），此处做别名转回 v8::
template <class T>
using MaybeLocal = v8::MaybeLocal<T>;
template <class T>
using Maybe = v8::Maybe<T>;

// ===== New =====

// Nan::New("xx") → MaybeLocal<String>（pf.h:36、pf.cc:40,609 等经 .ToLocalChecked() 用）
inline v8::MaybeLocal<v8::String> New(const char* s) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::STRING;
	node->str = s;
	return v8::MaybeLocal<v8::String>(v8::Local<v8::String>(node));
}

// Nan::New(整数) → Local<Value>（Number）。pf.cc 中实参为 uint8_t/uint32_t/int32_t。
template <class T, class = std::enable_if_t<std::is_integral_v<T>>>
inline v8::Local<v8::Value> New(T n) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::NUMBER;
	node->num = static_cast<double>(n);
	return v8::Local<v8::Value>(node);
}

// Nan::New<v8::Array>(len)（pf.cc:481,570,575,586）、New<v8::Object>()（pf.cc:595）、
// New<v8::Boolean>(b)（pf.cc:599）——按标签分派。
template <class T>
struct NewTag {};
inline v8::Local<v8::Array> NewImpl(NewTag<v8::Array>, int /*length*/) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::ARRAY;
	return v8::Local<v8::Array>(node);
}
inline v8::Local<v8::Object> NewImpl(NewTag<v8::Object>) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::OBJECT;
	return v8::Local<v8::Object>(node);
}
inline v8::Local<v8::Boolean> NewImpl(NewTag<v8::Boolean>, bool b) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::BOOLEAN;
	node->b = b;
	return v8::Local<v8::Boolean>(node);
}
template <class T, class... Args>
inline auto New(Args&&... args) -> decltype(NewImpl(NewTag<T>{}, std::forward<Args>(args)...)) {
	return NewImpl(NewTag<T>{}, std::forward<Args>(args)...);
}

// Nan::Undefined()（pf.cc:42,509,566 等）。返回 Local<Value>，可被 Cast 成任意句柄。
inline v8::Local<v8::Value> Undefined() {
	return v8::Local<v8::Value>(kk_compat::t_arena->alloc());
}

// ===== To =====

namespace impl {
template <class T>
struct Tag {};
} // namespace impl

inline v8::MaybeLocal<v8::Object> ToDispatch(v8::Local<v8::Value> v, impl::Tag<v8::Object>) {
	// Nan::To<v8::Object>(val).ToLocalChecked()（pf.h:35,78-80,243）
	v8::Value* node = v.v ? v.v : kk_compat::t_arena->alloc();
	return v8::MaybeLocal<v8::Object>(v8::Local<v8::Object>(node));
}
inline v8::Maybe<uint32_t> ToDispatch(v8::Local<v8::Value> v, impl::Tag<uint32_t>) {
	// Nan::To<uint32_t>(val).FromJust()（pf.h:36-37,79-80,244）。值均由本层以
	// double 精确承载 u32，static_cast 语义与 JS ToUint32 在该值域一致。
	uint32_t n = v.v ? static_cast<uint32_t>(v.v->num) : 0;
	return v8::Maybe<uint32_t>(n);
}
template <class T>
inline auto To(v8::Local<v8::Value> v) -> decltype(ToDispatch(v, impl::Tag<T>{})) {
	return ToDispatch(v, impl::Tag<T>{});
}

// ===== Get / Set =====

// Nan::Get(obj, "key")（pf.h:36-37,79-80,244-245、pf.cc:608-610）
inline v8::MaybeLocal<v8::Value> Get(v8::Local<v8::Object> obj, v8::Local<v8::Value> key) {
	v8::Value* found = nullptr;
	if (obj.v != nullptr && key.v != nullptr) {
		found = obj.v->get_prop(key.v->str.c_str());
	}
	if (found == nullptr) {
		found = kk_compat::t_arena->alloc(); // undefined
	}
	return v8::MaybeLocal<v8::Value>(v8::Local<v8::Value>(found));
}

// Nan::Get(array, index)（pf.cc:480-481,607-608）
inline v8::MaybeLocal<v8::Value> Get(v8::Local<v8::Object> obj, uint32_t index) {
	v8::Value* found = nullptr;
	if (obj.v != nullptr && index < obj.v->elems.size()) {
		found = obj.v->elems[index];
	}
	if (found == nullptr) {
		found = kk_compat::t_arena->alloc(); // undefined
	}
	return v8::MaybeLocal<v8::Value>(v8::Local<v8::Value>(found));
}

// Nan::Set(obj, "key", value)（pf.cc:596-599）
inline void Set(v8::Local<v8::Object> obj, v8::Local<v8::Value> key, v8::Local<v8::Value> val) {
	if (obj.v == nullptr || key.v == nullptr) {
		return;
	}
	for (auto& prop : obj.v->props) {
		if (prop.first == key.v->str) {
			prop.second = val.v;
			return;
		}
	}
	obj.v->props.emplace_back(key.v->str, val.v);
}

// Nan::Set(array, index, value)（pf.cc:578,588）——按下标可增长（v8 数组语义）
inline void Set(v8::Local<v8::Object> obj, uint32_t index, v8::Local<v8::Value> val) {
	if (obj.v == nullptr) {
		return;
	}
	if (index >= obj.v->elems.size()) {
		obj.v->elems.resize(index + 1);
	}
	obj.v->elems[index] = val.v;
}

// ===== Call / 异常 =====

// Nan::Call(fn, recv, argc, argv)（pf.cc:42）。fn 必须是 kk_ffi.cc 注入的 FUNCTION
// 节点：把 (xx, yy) 两个数值实参转交 C 回调，再把返回码包成 undefined/false/Uint8Array
// ——与官方 JS bridge（driver/lib/path-finder.js:83-92）返回给 native 的形态一致。
inline v8::MaybeLocal<v8::Value> Call(v8::Local<v8::Function> fn, v8::Local<v8::Object> /*recv*/,
	int argc, v8::Local<v8::Value> argv[]) {
	v8::Value* result = kk_compat::t_arena->alloc();
	if (fn.v != nullptr && fn.v->type == v8::Value::FUNCTION && fn.v->fn_ptr != nullptr && argc >= 2) {
		uint32_t xx = static_cast<uint32_t>(argv[0].v ? argv[0].v->num : 0);
		uint32_t yy = static_cast<uint32_t>(argv[1].v ? argv[1].v->num : 0);
		const uint8_t* matrix = nullptr;
		int rc = fn.v->fn_ptr(fn.v->fn_user, static_cast<uint8_t>(xx), static_cast<uint8_t>(yy), &matrix);
		if (rc == 1) {
			result->type = v8::Value::BOOLEAN;
			result->b = false;
		} else if (rc == 2) {
			result->type = v8::Value::TARRAY;
			result->tarray_data = matrix;
			result->tarray_len = 2500;
		}
	}
	return v8::MaybeLocal<v8::Value>(v8::Local<v8::Value>(result));
}

// Nan::ThrowError("...")（pf.cc:33）：置挂起标志。官方随后 throw js_error，
// 由 pf.cc:563-567 捕获并返回 Undefined。
inline void ThrowError(const char* /*message*/) { kk_compat::t_pending_js_exception = true; }

// Nan::TryCatch（pf.cc:38-46）。本兼容层的回调不产生 JS 异常，HasCaught() 恒 false；
// 保留类型以通过编译并维持官方控制流形状。
class TryCatch {
public:
	TryCatch() = default;
	bool HasCaught() const { return kk_compat::t_pending_js_exception; }
	void ReThrow() const {}
};

// Nan::TypedArrayContents<uint8_t>（pf.cc:54-56,610）：从 Uint8Array 节点取
// (数据指针, 长度)。*tac 返回 T*。
template <class T>
class TypedArrayContents {
public:
	template <class S>
	explicit TypedArrayContents(v8::Local<S> val) {
		if (val.v != nullptr && val.v->type == v8::Value::TARRAY) {
			data_ = const_cast<T*>(reinterpret_cast<const T*>(val.v->tarray_data));
			length_ = val.v->tarray_len;
		}
	}
	size_t length() const { return length_; }
	T* operator*() const { return data_; }

private:
	T* data_ = nullptr;
	size_t length_ = 0;
};

} // namespace Nan

#endif // KK_PATHFINDER_COMPAT_NAN_H_
