// 意图压制夹具（G07.b）：按场景 memory 的 mode/prefix 用玩家面原语
// 发射扩展意图。两意图都设绝对值，期望每 tick 稳定：both=bump(3)+
// mark(7)，mark 被 bump 压制 → 3；alone=只 mark(7) → 7；invalid=
// bump(99 越界被 schema 丢弃但键存在)+mark(7)，压制只看存在性 → 0；
// free 同 both 但未声明压制（键序 bump 先置 3、mark 后置 7）→ 7；
// combined=两包同选，各类型一对象 → suppress 侧 3、free 侧 7。
module.exports.loop = function () {
  const prefix = Memory.prefix;
  const emit = __kkEmitExtensionIntent;
  const mode = Memory.mode;
  if (mode === 'combined') {
    // 组合选择：两包同计划共存，压制按包内声明各自生效（3 与 7）。
    const a = Game.rooms['W1N1'].find(FIND_COUNTERS)[0];
    const b = Game.rooms['W1N1'].find(FIND_FREE_COUNTERS)[0];
    emit('suppress:bump', a.id, { amount: 3 });
    emit('suppress:mark', a.id, { mark: 7 });
    emit('free:bump', b.id, { amount: 3 });
    emit('free:mark', b.id, { mark: 7 });
    return;
  }
  const finder = prefix === 'free:' ? FIND_FREE_COUNTERS : FIND_COUNTERS;
  const counters = Game.rooms['W1N1'].find(finder);
  const target = counters && counters[0];
  if (!target) return;
  if (mode === 'both' || mode === 'free') {
    emit(prefix + 'bump', target.id, { amount: 3 });
    emit(prefix + 'mark', target.id, { mark: 7 });
  } else if (mode === 'alone') {
    emit(prefix + 'mark', target.id, { mark: 7 });
  } else if (mode === 'invalid') {
    emit(prefix + 'bump', target.id, { amount: 99 });
    emit(prefix + 'mark', target.id, { mark: 7 });
  }
};
