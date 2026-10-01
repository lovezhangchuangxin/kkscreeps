// 提供 pf.cc/pf.h 使用的 Nan/v8 接口子集，不嵌入 JavaScript 引擎。
// 值节点由当前 FFI 调用的 Arena 分配；Local 只是非拥有指针，不能在调用结束后使用。
// 嵌套调用切换 Arena 并在返回时恢复，外层节点仍存活。TypedArray 节点借用 Rust
// 缓冲，矩阵保持有效到搜索结束，地形只需有效到载入结束。
// 函数节点通过 C 回调取房间决策；异常状态由 kk_ffi.cc 的 ArenaGuard 按调用层保存。
// IsExecutionTerminating 恒为 false，本层没有执行超时；max_ops 不限制回调耗时。
// 来源版本见 src/lib.rs；这里只覆盖上游寻路源码实际使用的值操作，不实现完整 V8 语义。

#ifndef KK_PATHFINDER_COMPAT_NAN_H_
#define KK_PATHFINDER_COMPAT_NAN_H_

// pf.h/pf.cc 使用这些类型却经 nan.h 获得声明，兼容层须提供对应头文件。
#include <cstdint>
#include <limits>

#include <cstring>
#include <deque>
#include <string>
#include <utility>
#include <vector>

namespace v8 {

// 值的行为由 type 标签决定；本层的句柄类型只服务上游接口的编译要求。
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
	// FUNCTION 借用回调上下文，返回码约定见 kk_ffi.h。
	int (*fn_ptr)(void*, uint8_t, uint8_t, const uint8_t**, size_t*) = nullptr;
	void* fn_user = nullptr;

	bool IsUndefined() const { return type == UNDEFINED; }
	bool IsBoolean() const { return type == BOOLEAN; }
	bool IsFalse() const { return type == BOOLEAN && !b; }
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

// 非拥有句柄；模板转换限制为声明的类型继承关系，生命周期由 ArenaGuard 管理。
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

	// 上游将 Undefined 转作回调接收者；Cast 只改变句柄类型，不创建对象。
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

// deque 在追加节点时保持既有节点地址稳定，已有 Local 指针无需更新。
struct Arena {
	std::deque<v8::Value> nodes;
	v8::Value* alloc() {
		nodes.emplace_back();
		return &nodes.back();
	}
};

inline thread_local Arena* t_arena = nullptr;
// 只表示当前 FFI 调用的异常；嵌套返回时恢复外层状态，不能残留内层错误。
inline thread_local bool t_pending_js_exception = false;
// ThrowError 的消息指针；回调异常没有文本，ReThrow 会清空此值。
inline thread_local const char* t_pending_js_error_message = nullptr;

} // namespace kk_compat

namespace Nan {

template <class T>
using MaybeLocal = v8::MaybeLocal<T>;
template <class T>
using Maybe = v8::Maybe<T>;

inline v8::MaybeLocal<v8::String> New(const char* s) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::STRING;
	node->str = s;
	return v8::MaybeLocal<v8::String>(v8::Local<v8::String>(node));
}

template <class T, class = std::enable_if_t<std::is_integral_v<T>>>
inline v8::Local<v8::Value> New(T n) {
	v8::Value* node = kk_compat::t_arena->alloc();
	node->type = v8::Value::NUMBER;
	node->num = static_cast<double>(n);
	return v8::Local<v8::Value>(node);
}

template <class T>
struct NewTag {};
// 上游创建数组后按下标 Set，或直接返回空数组；初始长度不用于独立读取。
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

inline v8::Local<v8::Value> Undefined() {
	return v8::Local<v8::Value>(kk_compat::t_arena->alloc());
}

namespace impl {
template <class T>
struct Tag {};
} // namespace impl

inline v8::MaybeLocal<v8::Object> ToDispatch(v8::Local<v8::Value> v, impl::Tag<v8::Object>) {
	v8::Value* node = v.v ? v.v : kk_compat::t_arena->alloc();
	return v8::MaybeLocal<v8::Object>(v8::Local<v8::Object>(node));
}
inline v8::Maybe<uint32_t> ToDispatch(v8::Local<v8::Value> v, impl::Tag<uint32_t>) {
	// 输入均为包装层创建的 u32 数值；不实现一般 JS 值的 ToUint32 转换。
	uint32_t n = v.v ? static_cast<uint32_t>(v.v->num) : 0;
	return v8::Maybe<uint32_t>(n);
}
template <class T>
inline auto To(v8::Local<v8::Value> v) -> decltype(ToDispatch(v, impl::Tag<T>{})) {
	return ToDispatch(v, impl::Tag<T>{});
}

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

inline void Set(v8::Local<v8::Object> obj, uint32_t index, v8::Local<v8::Value> val) {
	if (obj.v == nullptr) {
		return;
	}
	if (index >= obj.v->elems.size()) {
		obj.v->elems.resize(index + 1);
	}
	obj.v->elems[index] = val.v;
}

// pf.cc 同步提供两个地图坐标。返回码 3 同时置异常标志和空 MaybeLocal，
// 使 pf.cc 的 TryCatch 中止本次搜索；0/1/2 分别转换为 Undefined/false/字节数组。
// 矩阵只借用指针并传递真实长度，不在此复制或检查 2500 字节。
inline v8::MaybeLocal<v8::Value> Call(v8::Local<v8::Function> fn, v8::Local<v8::Object> /*recv*/,
	int argc, v8::Local<v8::Value> argv[]) {
	if (fn.v != nullptr && fn.v->type == v8::Value::FUNCTION && fn.v->fn_ptr != nullptr && argc >= 2) {
		uint32_t xx = static_cast<uint32_t>(argv[0].v ? argv[0].v->num : 0);
		uint32_t yy = static_cast<uint32_t>(argv[1].v ? argv[1].v->num : 0);
		const uint8_t* matrix = nullptr;
		size_t matrix_len = 0;
		int rc = fn.v->fn_ptr(fn.v->fn_user, static_cast<uint8_t>(xx), static_cast<uint8_t>(yy),
			&matrix, &matrix_len);
		if (rc == 3) {
			kk_compat::t_pending_js_exception = true;
			return v8::MaybeLocal<v8::Value>(); // 空返回 = JS 抛错（真实 v8 语义）
		}
		v8::Value* result = kk_compat::t_arena->alloc();
		if (rc == 1) {
			result->type = v8::Value::BOOLEAN;
			result->b = false;
		} else if (rc == 2) {
			result->type = v8::Value::TARRAY;
			result->tarray_data = matrix;
			result->tarray_len = matrix_len; // pf.cc 再检查是否为 2500 字节
		}
		return v8::MaybeLocal<v8::Value>(v8::Local<v8::Value>(result));
	}
	v8::Value* result = kk_compat::t_arena->alloc();
	return v8::MaybeLocal<v8::Value>(v8::Local<v8::Value>(result));
}

// 地形缺失时，pf.cc 先设置消息，再抛出它自己的 js_error 中止搜索。
inline void ThrowError(const char* message) {
	kk_compat::t_pending_js_exception = true;
	kk_compat::t_pending_js_error_message = message;
}

// pf.cc 在调用房间回调后检查此标志，并通过 ReThrow 保持错误状态。
// 当前层由 ArenaGuard 隔离；内层返回错误不会被误判为当前回调抛错。
class TryCatch {
public:
	TryCatch() = default;
	bool HasCaught() const { return kk_compat::t_pending_js_exception; }
	void ReThrow() const {
		// 回调异常没有 ThrowError 的文本，不能沿用此前其他错误的消息。
		kk_compat::t_pending_js_error_message = nullptr;
	}
};

// 上游只以 uint8_t 读取字节。本层借用缓冲，不拥有或延长其生命周期；
// 尽管接口返回 T*，pf.cc 只读矩阵和地形字节。
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
