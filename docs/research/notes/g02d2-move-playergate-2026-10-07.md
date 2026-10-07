# G02.d2 移动玩家门双端冻结与寻路边界钉死（2026-10-07）

基线 `f84a2b4`。本记录交付疲劳/MOVE 玩家门的规则化双端同源与寻路边界回归证据，G02 父项仍部分完成。现行契约见[玩家预检规则](../../architecture/player-precheck-rules.md)的[移动玩家门](../../architecture/player-precheck-rules.md#移动玩家门g02d2)节，示例见 [marathon/legless](../../../compat/fixtures/mod-move-playergate/README.md)。机器结果见[同名证据](g02d2-move-playergate-2026-10-07.json)。

## 实施与复审

两路盘点定界后照 G07.a 四层复制：`builtin:creep.move.fatigueLimit`（默认 0、域 0..=u32::MAX）与 `builtin:creep.move.minActiveMoves`（默认 1、域 0..=50）入冻结规则，`MoveGate::from_plan` 快照经 install 双路径进 Engine/Plane，`can_move` 按快照判定仲裁原生预填；常量表新增 `MOVE_FATIGUE_LIMIT`/`MOVE_MIN_ACTIVE_MOVES`（427→429，gen-constants + kk-core 键数门 + catalog 逐键比对与规则数 29→31 三道门同步），setup.rs canary/真实玩家双通道注入，creep.js `move()`/`moveTo()` 四门改读常量（moveByPath/`_move` 缓存臂经 move() 单点收口自动覆盖）。单一 override 双端生效，消除「准入槽放行疲劳者但方向形态在 API 层即返 ERR_TIRED」的撕裂。被拉豁免维持 `move(target)` 结构通道不进规则面；visualizePathStyle 豁免等官方怪癖保持。revision v15→v16，槽仍 20，无新持久写入域（规则经 plan_hash 覆盖，检查点恢复等值核对兜底）。

寻路面按盘点结论不做实现、只钉边界：官方 moveTo 沿 search 部分路径靠近并返回 OK（非 ERR_NO_PATH），钉住的边界是步进永不越墙列、findPath 止于墙前一格；ghost 放行下同 tick 玩家 PathFinder 仍绕行原生墙格而结算侧真实穿墙（双断言互为对照）。放行决策按请求粒度、在结算段产生，寻路地形在 tick 开始注入——时序与粒度均不允许感知，这是设计约束而非待补能力。声明式寻路规则（计划级可走性，仿建筑 BuildingPathing 先例：pathfinder 地形包/room.js 基线矩阵/static_obstacles 三点同源）登记为 G02.d3 设计，未实现；tick 级放行决策与服务器 JS 回调永久不进玩家寻路。

本轮自审检查了：默认门≡旧行为（既有全套引擎/runner/runtime 测试在默认快照下零改动通过）、两门独立叠加、被拉豁免不受规则影响、无 MOVE 移动疲劳照常累积且永不恢复（玩法自担）、方向形态 TIRED/NO_BODYPART 返回码首次直接钉死（此前仅 moveTo 形态有断言）、规则型包无处理器不产生 namespace state（首版场景误断言 mod_states applied 即红，删断言改由 plan_hash 四身份承担——与 combat-ranges 先例一致）、sim 场景不支持预置疲劳故用自然疲劳建模（runner 动一停一）。当前无已知阻塞项，完整门禁与复审待执行。

## 已执行定向验收

- `cargo test -p kk-engine --lib admission`：13 通过（含新增 `move_gate_rules_drive_native_admission_prefill`：默认≡旧硬编码、两覆盖、组合、预填链路）。
- `cargo test -p kk-runner --test sandbox_tests`：全绿（含新增 `move_gate_constants_impl` 默认返回码/覆盖平铺/两门独立与 `move_to_never_paths_through_walls_impl` 墙列边界）。
- `cargo test -p kk-runtime --test mod_chain_tests move_gate`：3 通过（疲劳连走/无件行走/组合叠加，双通道）；`move_static` 10 通过（新增 ghost 双断言 `pathfinder_stays_native_blind_to_static_allowance`）。
- `cargo test -p kk-cli --test sim_tests move_playergate`：1 通过；四场景 × 双通道、保存退出码 2/新进程加载续跑等值、回放 passed，99.03 秒。

完整门禁、三路复审与 release ratchet 结果见下节。未修改或新增 Rust doctest。

## 复审清偿与最终结论（2026-10-07）

三路分域复审（引擎/规则面、玩家面/注入链、测试/夹具/文档）核心声称全部核实：默认 ≡ 旧硬编码有「既有断言零改动通过」的逐行证据、gen-constants 重生成产物与提交内容逐字节一致、双通道注入与战斗射程先例完全同构、四门比较方向与结算侧恰为补集（无 off-by-one）、常量键链五环闭合、can_move 唯一生产消费点在 prefill。复审发现并全部清偿：**P1** `prelude_and_room_forms.rs` 增至 1022 行超源码规模门——常量注入与门族三函数拆出 `constant_gate_forms.rs`（847+178 行，注册点不变）；P3×8——kk-core 键数门注释「4 个本地常量」改 6、mod-move-precheck.md revision 演进口径（现 v16）、mod-move-admission.md 槽数 18→20、creep-resource-slots.md 规则/槽数陈旧数字、pathfinder_family.rs 头注释残留「ERR_NO_PATH」盘点期假设（与行内注释及断言矛盾）、move_gate.rs 默认臂 `return 0` 在全局 eval 下是语法错误（首轮实为无效轮）改为干净首轮并断言返回码、sim README 披露 final 比较剥离 CPU bucket、admission 测试 m2/m3 改用各自 room。

流程教训：首轮完整门禁经 `| tail` 管道运行，后台通知的 exit 0 实为 tail 的退出码——源码规模段实际已红，被测试侧复审子代理以实测抓出；最终轮显式落盘退出码确认通过。

- 完整门禁 `pnpm run check`（无参数，清偿后最终树）：CHECK_EXIT=0；Rust 1219 通过/12 既有忽略、compat JS 15、renderer 1、文档链接 334 文件、源码规模 632 文件全过。
- release ratchet：1918/2914 通过（floor 1918 恰达标）、相对 HEAD~1 基线零转红；两条「豁免条目未命中」警告为历史陈旧条目提示（同 G02.d1），非本批引入。
- 提交状态：用户确认后已随本批提交；提交 ID 由下次接手从 Git 补录。

## 剩余

G02.d3 声明式寻路规则：设计已登记（数据面=计划级可走性规则；消费面=kk-pathfinder 地形包、room.js `getPathfindingGrid2` 基线矩阵、engine `static_obstacles.rs` 原生默认三点同源；开放问题=玩家可见地形形状 `getTerrain().get` 是否随规则改写）。孵化门槛、完整仲裁替换、障碍槽第二次预演仍属后续切片。
