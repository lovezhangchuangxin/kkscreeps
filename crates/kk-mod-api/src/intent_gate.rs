//! 扩展意图的可执行性门：声明生成（typegen）与引擎装配（install）共用，
//! 保证意图描述符的支持范围同一口径；不代替执行入口与绑定校验。豁免按
//! 意图键命名空间判定（builtin 意图由 Rust native 执行器消费），由
//! 调用方先行过滤；入口槽解析与阶段检查留在引擎侧（依赖槽句柄）。
use crate::{ArgumentConversion, IntentAuthority, IntentDescriptor, IntentScope, Merge};

/// 逐项拦截未接线的执行形态；错误文案是装配期拒绝的一部分，引擎
/// 测试按子串断言，修改须两侧同步。
pub fn executable_gate(intent: &IntentDescriptor) -> Result<(), String> {
    if intent.scope != IntentScope::Object {
        return Err(format!(
            "动作 {} 的 {} 域执行通道未接线（当前只接线对象域）",
            intent.key,
            match intent.scope {
                IntentScope::Room => "房间",
                IntentScope::Player => "玩家",
                _ => "世界",
            }
        ));
    }
    if !matches!(intent.merge, Merge::Last) {
        return Err(format!(
            "动作 {} 声明队列合并模式，该模式的执行装配未接线",
            intent.key
        ));
    }
    if !matches!(intent.conversion, ArgumentConversion::Strict) {
        return Err(format!(
            "动作 {} 声明非严格参数转换，该形态的执行装配未接线",
            intent.key
        ));
    }
    if intent.authority != IntentAuthority::Player {
        return Err(format!(
            "动作 {} 声明非玩家授权，服务端发射通道未接线",
            intent.key
        ));
    }
    // 内建类型接收者（如 builtin:link）：freeze/player_surface 能通过
    //（类型目录含内建类型），但 ext 派发只认 Mod 对象——不拦就是声明
    // 通过、运行期静默永不执行的死码，按本门「拦截未接线执行形态」
    // 的职责显式拒绝（G07.b 复审清偿）。
    if let Some(receiver) = intent.receiver.as_ref()
        && receiver.key.namespace() == "builtin"
    {
        return Err(format!(
            "动作 {} 的接收者是内建类型 {}，内建对象上的扩展意图派发未接线",
            intent.key, receiver.key
        ));
    }
    // 压制关系自 G07.b 起可执行：引擎侧在 ext 派发点做存在性判定
    //（settle::mod_intents，与 vanilla Action::executes 同语义），
    // 冻结期校验压制动作须同 scope 同接收者（freeze）。
    Ok(())
}
