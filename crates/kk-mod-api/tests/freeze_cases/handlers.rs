use super::*;

#[test]
fn handler_order_replace_and_reverse_after_are_explicit() {
    let mut c = module("test");
    for kind in [HandlerKind::Before, HandlerKind::Wrap, HandlerKind::After] {
        let suffix = format!("{kind:?}").to_lowercase();
        handler(
            &mut c,
            &format!("a.{suffix}"),
            kind,
            &[&format!("test:z.{suffix}")],
        );
        handler(&mut c, &format!("z.{suffix}"), kind, &[]);
    }
    handler(&mut c, "replacement", HandlerKind::Replace, &[]);
    let plan = plan(vec![c, builtin::link()]).unwrap();
    let slot = plan
        .slots()
        .iter()
        .find(|s| s.key == key("builtin:link.transfer"))
        .unwrap();
    assert_eq!(
        slot.dispatch,
        Dispatch::Chained {
            before: vec![key("test:z.before"), key("test:a.before")],
            wraps: vec![key("test:z.wrap"), key("test:a.wrap")],
            center: key("test:replacement"),
            after: vec![key("test:a.after"), key("test:z.after")],
        }
    );
}

#[test]
fn handler_cycles_missing_dependencies_and_multiple_replacements_fail_at_freeze() {
    let mut c = module("test");
    handler(&mut c, "a", HandlerKind::Wrap, &["test:b"]);
    handler(&mut c, "b", HandlerKind::Wrap, &["test:a"]);
    assert!(
        plan(vec![builtin::link(), c])
            .unwrap_err()
            .message
            .contains("有环")
    );
    let mut c = module("test");
    handler(&mut c, "a", HandlerKind::Wrap, &["test:missing"]);
    assert!(
        plan(vec![builtin::link(), c])
            .unwrap_err()
            .message
            .contains("未知处理器依赖")
    );
    let mut c = module("test");
    handler(&mut c, "a", HandlerKind::Wrap, &["test:b"]);
    handler(&mut c, "b", HandlerKind::Before, &[]);
    assert!(
        plan(vec![builtin::link(), c])
            .unwrap_err()
            .message
            .contains("同一种 kind")
    );
    let mut c = module("test");
    handler(&mut c, "a", HandlerKind::Replace, &[]);
    handler(&mut c, "b", HandlerKind::Replace, &[]);
    assert!(
        plan(vec![builtin::link(), c])
            .unwrap_err()
            .message
            .contains("replace 处理器冲突")
    );
}

#[test]
fn invalid_intent_scope_merge_conversion_and_suppression_are_rejected() {
    let mut c = builtin::link();
    c.intents[0].scope = IntentScope::Room;
    error(c, "接收者");
    let mut c = builtin::link();
    c.intents[0].merge = Merge::Queue { max_len: 0 };
    error(c, "队列上限");
    let mut c = builtin::link();
    c.intents[0].conversion = ArgumentConversion::Native(key("builtin:missing"));
    error(c, "未知绑定");
    let mut c = builtin::link();
    c.intents[0].suppressed_by.insert(key("builtin:missing"));
    error(c, "未知压制动作");
    let mut c = builtin::link();
    let self_key = c.intents[0].key.clone();
    c.intents[0].suppressed_by.insert(self_key);
    error(c, "其他动作");
}
