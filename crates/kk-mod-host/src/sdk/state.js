// 每个 realm 独立持有注册表；内部模块共享实时绑定，不跨 isolate。
export let sealed = false;
export const bindings = new Map();
export const declines = new WeakMap();
export let effectiveRules = new Map();
export let rulesInstalled = false;
export function sealState() { sealed = true; }
export function setRules(rules) { effectiveRules = rules; rulesInstalled = true; }
