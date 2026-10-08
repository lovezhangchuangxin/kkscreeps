// 仅复用自有数据属性，不能把 __proto__ 等继承值当成待刷新容器。
function ownValue(object, key) {
  const descriptor = Object.getOwnPropertyDescriptor(object, key);
  return descriptor && Object.hasOwn(descriptor, 'value') ? descriptor.value : undefined;
}

// Rust 候选是 JSON 树；旧 JS 图中的共享容器只能复用一次，其余路径
// 改用独立候选值，避免第二次刷新覆盖第一次刷新。独立容器保持旧引用。
// keys 用于上下文记录：只更新权威成员，保留处理器附加的本地属性。
function refreshValue(current, fresh, reused, keys) {
  if (
    current === null ||
    fresh === null ||
    typeof current !== 'object' ||
    typeof fresh !== 'object' ||
    Array.isArray(current) !== Array.isArray(fresh)
  ) {
    return fresh;
  }
  if (reused.has(current)) return fresh;
  reused.add(current);
  if (keys === undefined) {
    for (const key of Object.keys(current)) {
      if (!Object.hasOwn(fresh, key)) delete current[key];
    }
  }
  if (Array.isArray(current)) current.length = fresh.length;
  for (const key of keys ?? Object.keys(fresh)) {
    Object.defineProperty(current, key, {
      value: refreshValue(ownValue(current, key), fresh[key], reused),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }
  return current;
}

export function refreshContext(ctx, fresh) {
  // state/args/target 共用一次遍历，跨上下文字段的别名也不能重复复用。
  const reused = new WeakSet([ctx]);
  for (const key of ['tick', 'room', 'state', 'args']) {
    Object.defineProperty(ctx, key, {
      value: refreshValue(ownValue(ctx, key), fresh[key], reused),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }
  const target = refreshValue(ownValue(ctx, 'target'), fresh.target, reused, ['id', 'fields']);
  Object.defineProperty(ctx, 'target', {
    value: target,
    writable: true,
    enumerable: true,
    configurable: true,
  });
}
