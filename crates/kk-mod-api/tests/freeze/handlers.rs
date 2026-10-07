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
    // 压制动作在不同接收者上（G07.b 清偿：钉死同接收者分支）——需要
    // 两个类型：克隆 link 类型为 builtin:panel，意图挂其上，link.transfer
    // 声明被它压制 → 冻结拒。
    let mut c = builtin::link();
    let mut panel_intent = c.intents[0].clone();
    panel_intent.key = key("builtin:panel.tap");
    panel_intent.slot = key("builtin:panel.tap.slot");
    panel_intent.suppressed_by = BTreeSet::new();
    let mut panel_slot = c.slots[0].clone();
    panel_slot.key = key("builtin:panel.tap.slot");
    let mut panel_type = c.object_types[0].clone();
    panel_type.key = key("builtin:panel");
    panel_intent.receiver = Some(TypeRef {
        key: key("builtin:panel"),
        schema_version: 1,
    });
    c.intents.push(panel_intent);
    c.slots.push(panel_slot);
    c.object_types.push(panel_type);
    c.intents[0].suppressed_by.insert(key("builtin:panel.tap"));
    error(c, "同一接收者");
    // 互压环：A 压 B、B 压 A——存在性判定下齐发双双静默丢弃，冻结拒。
    let mut c = builtin::link();
    let mut second = builtin::link();
    second.intents[0].key = key("builtin:second");
    second.intents[0].slot = key("builtin:second.slot");
    second.slots[0].key = key("builtin:second.slot");
    second.intents[0]
        .suppressed_by
        .insert(c.intents[0].key.clone());
    c.intents[0]
        .suppressed_by
        .insert(second.intents[0].key.clone());
    // link() 的两条意图共用接收者 builtin:link：直接把 second 追加进 c。
    c.intents.push(second.intents[0].clone());
    c.slots.push(second.slots[0].clone());
    error(c, "压制关系成环");
}

#[test]
fn suppression_rejects_long_cycles_but_accepts_chains() {
    for count in [3, 8] {
        let mut c = builtin::link();
        let template = c.intents[0].clone();
        for i in 0..count {
            let mut intent = template.clone();
            intent.key = key(&format!("builtin:cycle{i}"));
            intent.suppressed_by =
                BTreeSet::from([key(&format!("builtin:cycle{}", (i + 1) % count))]);
            c.intents.push(intent);
        }
        error(c.clone(), "压制关系成环");
        c.intents.last_mut().unwrap().suppressed_by.clear();
        assert!(plan(vec![c]).is_ok(), "移除一条边后长链可冻结");
    }
}
