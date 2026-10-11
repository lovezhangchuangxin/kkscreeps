//! 原生候选位置索引与 ID 游标的动态对象日志复原。
use super::*;
use kk_engine::settle::objects;

#[test]
fn object_mutation_journal_restores_all_positions_and_creation_cursor() {
    let mut f = Fixture::new(3, kk_core::Terrain::plains());
    let actor = place_counter(&mut f, 10, 12, "p1", 1);
    let peer = place_counter(&mut f, 20, 12, "p1", 2);
    let colocated = place_counter(&mut f, 10, 12, "p1", 3);
    let plan = frozen(counter_contribution(&[]));
    let process: ExtSimple = Box::new(move |ctx, id, _| {
        let mut w = ctx.borrow_mut();
        let before_count = w.room.objects.len();
        let snapshot = w.snapshot_mod_objects();
        objects::update(&mut w, "extmod", id, &json!({"x":11,"value":7}))?;
        objects::update(&mut w, "extmod", peer, &json!({"x":21}))?;
        assert!(w.index.at(11, 12).contains(&id));
        assert!(!w.index.at(10, 12).contains(&id));
        objects::remove(&mut w, "extmod", peer)?;
        let created = objects::create(
            &mut w,
            "extmod",
            "extmod:counter",
            &json!({"owner":"p1","x":30,"y":12}),
        )?;
        let created_id = ObjId::parse(&created.id).unwrap();
        objects::update(&mut w, "extmod", created_id, &json!({"x":31}))?;
        objects::remove(&mut w, "extmod", created_id)?;
        objects::create(
            &mut w,
            "extmod",
            "extmod:counter",
            &json!({"owner":"p1","x":32,"y":12}),
        )?;
        w.restore_chain_snapshot(snapshot);
        assert_eq!(w.room.objects.len(), before_count);
        assert_eq!(w.index.at(10, 12), &[id, colocated]);
        assert_eq!(w.index.at(20, 12), &[peer]);
        for x in [11, 21, 30, 31, 32] {
            assert!(w.index.at(x, 12).is_empty(), "残留位置 {x}");
        }
        assert_eq!(w.mod_fields(id).unwrap()["value"], 1);
        let retry = objects::create(
            &mut w,
            "extmod",
            "extmod:counter",
            &json!({"owner":"p1","x":30,"y":12}),
        )?;
        assert_eq!(retry.id, created.id);
        Ok(())
    });
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::from([(Key::new("extmod:increment.process").unwrap(), process)]),
    )
    .unwrap();
    settle_quiet(
        &mut f,
        ext_batch("p1", actor, json!({"amount":1})),
        std::sync::Arc::new(dispatch),
        plan,
    )
    .unwrap();
}
