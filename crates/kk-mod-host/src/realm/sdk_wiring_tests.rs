//! 声明真实宿主 op 与 SDK 静态调用的双向接线门禁。
use super::*;
use std::collections::BTreeSet;

#[test]
fn every_sdk_op_is_registered_and_every_host_op_has_a_sdk_consumer() {
    let extension = kk_mod_host::init();
    let registered: BTreeSet<_> = extension.ops.iter().map(|op| op.name.to_string()).collect();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sdk");
    let mut used = BTreeSet::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "js") {
            let source = std::fs::read_to_string(path).unwrap();
            for tail in source.split("ops.").skip(1) {
                let name: String = tail
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .collect();
                assert!(name.starts_with("op_mod_"), "未知 SDK op：{name}");
                used.insert(name);
            }
        }
    }
    assert_eq!(registered, used, "SDK 与宿主 op 必须双向覆盖");
}
