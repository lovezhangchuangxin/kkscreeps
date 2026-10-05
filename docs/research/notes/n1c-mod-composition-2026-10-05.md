# N1c：JS 处理器依赖与组合诊断（2026-10-05）

基于 b7ebafc，JS handler 选项 dependsOn 进入原有同槽/同 kind 冻结图；四种注册方法返回稳定处理器键，选项严格校验，转换 getter 可合法嵌套注册，捕获转换故障仍不能冻结部分计划。缺失、环、跨槽/跨 kind、多 replace 诊断补充相关绑定。

现行契约见[组合架构](../../architecture/mod-composition.md)，机器汇总见[JSON](n1c-mod-composition-2026-10-05.json)。交付前完成代码复审、修复及完整门禁。

## 实现与评审

ExecutionChain 从原候选链模块拆出，实际链安装与诊断共享过滤/中心回退选择。CompositionReport 保存来源、原始/冻结/活动链、有效规则数值与选中来源、handler/manifest 合并依赖及每包停用闭包；runtime 的手动/自动停用沿同一闭包计算。直接宿主要求完整停用掩码，缺依赖者在安装前拒绝并密封失败 realm。

复审将 handler 选项规范化读取放在声明 op 的转换期，保留原型 getter 上的 dependsOn，并用回归测试防止其被对象枚举静默丢失。

复审确保 execution=hosted 不因停用所有钩子就误变为 native：引擎仍持原冻结 Hosted 执行表，只是当前中心回退到 native。相同数值的多个覆盖通过冻结选择记录来源，不用数值相等猜选中者；停用不删除覆盖/schema/数据。诊断副本不会修改引擎。

公开 mod inspect 检查真实包/注册图、读取配置、显式选择规则覆盖及预览停用；没有世界，不推进 tick。输出与包根目录/配置输入分离，失败覆盖已验证输出位置的旧成功报告。sim 顶层 composition 展示实际运行诊断。CLI 原子 JSON 报告写入抽到独立 report 模块，与 sim 共用。

## 功能验收

定向 host 组合 7 项、runtime 组合 2 项、CLI inspect 3 项、CLI 双房组合场景 1 项通过。包含 forward/cross-package 依赖、四类处理器键、逆序 after、重复依赖归一、错误形状/拼写/getter、合法嵌套声明、缺失/循环/跨槽/kind/replace、相同数值选中来源、handler+manifest 传递停用、跨 kind 的 Mod 相互依赖组、所属槽停用、故障缓存重试、Native/Hosted 诊断和输入文件保护。旧 Mod 领域与工作区回归由完整门禁验证。

独立 base/z-gate/a-audit/dependent 包不使用专用 Rust 分支：a-audit 在注册序中早于 z-gate，通过依赖得到固定 before/wrap 顺序及逆序 after。双房双玩家每轮读取 ZA([C])az，2000 轮后各对象 value=2000、base visits=4000，玩家最后读取 value=1999。JSON/KKPV 比较最终原生世界、完整 Mod 对象、state、玩家 Memory 与组合报告。

## 原始报告

[JSON 场景](n1c-mod-composition-json-2026-10-05.json)、[KKPV 场景](n1c-mod-composition-kkpv-2026-10-05.json)、[组合检查](n1c-mod-inspect-2026-10-05.json)和[停用预览](n1c-mod-inspect-disabled-2026-10-05.json)记录同一 release CLI 的身份。已逐项核对两场景最终世界（含完整 Mod 对象、state、玩家 Memory）与 composition，以及未停用 inspect 的 composition 完全一致；预览停用 z-gate 得到 a-audit/dependent/z-gate，中心回退 base 原 process，规则覆盖保留。

## 完整门禁

重新构建 release CLI，显式以 KKSCREEPS_BIN 指定该二进制执行 pnpm run check：格式、workspace Clippy、Rust 754 项通过/11 项忽略、适配器 14 项通过、文档链接和源码规模检查通过。358 个主项目源码/测试文件均不超过 1000 行；组合与报告模块拆为独立文件。

固定 screeps-ok 提交 34a630efa42908c117c19c25d2350fa2387bd633，构建适配器后运行 ratchet：[完整结果](../../../compat/reports/kkscreeps.json)共 2914 项，1918 通过/89 失败/907 跳过，满足 floor 1918。与批次开始 HEAD 的所有用例身份及状态逐项相同；ratchet 对 HEAD~1 的转红/转绿均为 0，没有新增豁免。两条既有未命中豁免警告保留，不归因于本批次。

## 剩余范围

inspect 是注册与组合诊断，不证明所有冻结域已接入引擎，也不操作运行服务器、选择内容、迁移或提供逐 tick trace。JS 有效规则读取属于下一片 N1d；完整内容预设、冷恢复和持久输入仍未完成。report 的 PlanHash 不含停用掩码，比较执行还须核对 disabled_mods。

运行耗时仅诊断，采样可能与验证进程并发，不是 native/JS 或 xxscreeps 的受控性能比较。历史 N1a/N1b 记录保留当时接口与证据。
