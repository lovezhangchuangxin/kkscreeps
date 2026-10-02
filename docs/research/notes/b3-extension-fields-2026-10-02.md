# B.3 扩展字段与 JSON 参考

本片在 `5d23dcb`（B.2+注释整改）基础上落地对象级扩展字段全链：
声明 → 冻结 → 装配 → 处理器写入 → 候选提交 → 投影按权限裁切。
依据[领域与投影设计](../../designs/domain-projection.md) §3、§5 与 §8 的 B.3 行。

## 实现

- **字段挂载通道**（kk-mod-api）：`FieldMountDescriptor` 把本贡献命名空间
  的字段挂到其他贡献声明的已有类型上——类型主体归其命名空间所有，
  挂载只追加字段、不改类型归属。冻结校验目标类型存在、版本匹配、
  键不重复；挂载进入投影目录并参与计划指纹。
- **扩展目录**（`ExtensionCatalog`）：从冻结计划的投影目录提取非
  `builtin:` 绑定的字段。目录提取时拒绝条件权限（无投影判定入口）
  与非 Missing 默认值（目录不自动落值）。目录按类型键组织字段，
  存储与投影键均为字段稳定键全名（namespace:name），命名空间隔离
  使扩展键不能与内建字段冲突。
- **权威存储**（kk-world）：`RoomState.extensions` 保存对象 ID →
  字段键 → 值。随房间候选克隆与提交；`encode/decode_snapshot`
  当前不保存扩展（持久化格式冻结前必须补入，见限制）。
- **写入与校验**（kk-engine）：`Working::set_extension` /
  `remove_extension` 按目录查规则，值经 `validate::check_value`
  （类型、取值域、长度与元素限额、有限浮点——复用冻结层同源校验器）。
  目前只有 Generic 结构按 structureType 映射 `builtin:{structure_type}`
  类型键；其他对象族未接入扩展平面。`remove_obj` 同步清理扩展表。
- **发布前校验**：validate 阶段检查候选房间的全部扩展——字段必须在
  目录中声明（封住绕过写入门直改 `room.extensions` 的键），ObjectRef
  字段的值必须解析为 ID 且对象存在于本轮任一候选房（悬空引用
  丢弃整轮候选）。扩展字段绑定计入"已安装绑定"核对。
- **投影**（kk-world）：`world_json_for_viewer_with_extensions` 把
  扩展字段按权限裁切后放进对象 `ext` 键——Public 对看到该对象的
  玩家可见、Owner 仅对象主人、ServerOnly 不进任何玩家投影。
  目录为空时输出与既有投影逐字节相同。`WorldSource` trait 增加对应
  方法（必选，防实现者静默丢扩展），EmbedEngine 玩家投影已接引擎目录。
- **Plane 收拢**：`settle_room` 的三个平面参数（links/contrib/目录）
  收拢为 `Plane` 结构，Working 持单一 `plane` 字段。

## 验证

- 工作区测试 436 通过、0 失败、5 忽略（+8：kk-mod-api 3、
  settle_tests m2b_extension_family 4、kk-runtime 1；清偿后含
  挂载拒绝测试、dyn 分发非空目录测试）。
- 新测试族 `m2b_extension_family`（4 项，悬空引用用例为评审清偿后
  的收敛语义）：
  ①投影权限隔离——p2 经自有 creep 获得房间视野后（无视野时对象不
  出现、断言恒真，须先给视野），owner 见 charge/label/target、
  非 owner 只见 label/target、ServerOnly 对任何玩家不可见、空目录
  不输出 ext 键；②写入门校验——未声明键/取值域上限/字符串限额/
  类型不匹配/null（非 nullable）各拒绝一次；③悬空引用使整轮候选
  失败（Engine 级：Err + 已提交世界无扩展 + 时间未推进）；
  ④删除对象清理扩展表无残留 + 直改扩展表塞未声明键在发布前被拒。
- 变异击杀 ×4（改后红、还原后绿）：Owner 裁切改恒真；写入门跳过
  schema 校验；validate 引用存在性检查置真；remove_obj 不清理扩展。
- 完整 screeps-ok（重建 release 后）：1854 通过、153 既有失败、
  907 跳过；相对 HEAD 报告逐用例转红/转跳过/缺失均为 0，转绿 0。
- bot 三夹具：tooangel/overmind 各 2000 tick、wasm 10 tick 全部完成；
  硬中断与停跑均为 0；console error 计数与既有基线一致
  （tooangel 0、overmind 1 轮 tick 1 既有形态、wasm 0，
  读自 compat/reports/bot-*.json）。
- 本片未改结算热路径（扩展检查在 validate 且空目录早退），未做
  性能矩阵测量；默认路径的世界投影逐字节等价由棘轮零转红零转绿
  与 kk-world 51 项投影测试锁定。

## 限制与后续

- 二进制房间快照（`encode/decode_snapshot`）不保存扩展。丢失面：
  测试夹具导出（kk-storage-bench）、快照恢复测试路径、以及
  m2b_notify_family 的双引擎逐房快照对照（该对照是等价性 oracle，
  扩展参与时会静默漏比较）。E 批持久化格式冻结前必须补入
  （快照版本随 schema 演进，当前版本 1 不做向后兼容追加）。
- 房间/玩家/世界层的扩展状态（设计 §3 的四层）未接入，本片只做
  对象层；其余三层随消费者出现时挂 `PlayerState`/`World`。
- 玩家 API 读取扩展（prelude getter/包装器）与自定义动作通道归
  C 批（mod-plane-batches §3 C 行"counter 对象与动作"）；
  B.3 的投影面到玩家 JSON `ext` 键为止。
- 条件权限（Visibility::Conditional）的投影判定入口未提供，
  目录提取层拒绝声明；B.4 分组缓存/私有块时接入。
- ObjectRef 引用校验覆盖本轮全部候选房；悬空引用在发布前收敛——
  nullable 字段写 null、非 nullable 字段整项删除（与玩家侧
  getObjectById 对失效引用返回 null 同形），被引用对象销毁的
  tick 仍可提交。评审初版为"悬空拒绝整轮"，会卡死销毁场景，已改。
- 玩家侧 `ext` 的可见性判定用投影对象的 owner 字段；不可见房间的
  己方旗/工地破例对象不排除在 ext 附加循环外——当前不携带扩展是
  写入门只接 Generic 结构使然，非投影排除；工地族接入扩展平面时
  会开始携带 ext（破例对象只出现在 owner 本人投影，无跨玩家泄露）。
- B.4 前须定案的登记项：扩展值的单对象总载荷与字段数显式上限
  （当前只有 per-值限额，声明侧 max_bytes/max_len 亦无冻结期上限——
  玩家写入通道开放前必须补）；ObjectRef 的 TypeRef 目标类型运行期
  匹配（当前只查存在性）；投影循环按对象线性找房 O(对象×房间)，
  分片规模需 ID→房索引；tombstone/ruin 投影的 owner 写在 user 键
  与权威 object_owner 的差异（当前被 structureType 门挡住不可达）；
  Generic 原文 data 的 `ext` 键与扩展输出同名（当前无可达写者，
  冻结期保留该键或登记）。
