//! 包路径、依赖、导入和文件装载失败矩阵。
use super::*;

#[test]
fn manifest_failures() {
    let root = temp_root("manifest");
    /// (包名, 覆盖文件, 错误须包含的子串)
    type ManifestCase = (
        &'static str,
        Vec<(&'static str, &'static str)>,
        &'static str,
    );
    let cases: Vec<ManifestCase> = vec![
        (
            "unknown-field",
            vec![(
                "mod.json",
                r#"{"id":"unknown-field","version":"1.0.0","apiVersion":"^1.0.0","extra":1}"#,
            )],
            "mod.json 解析失败",
        ),
        (
            "bad-id",
            vec![(
                "mod.json",
                r#"{"id":"Bad-Id","version":"1.0.0","apiVersion":"^1.0.0"}"#,
            )],
            "不得为 builtin",
        ),
        (
            "builtin-id",
            vec![(
                "mod.json",
                r#"{"id":"builtin","version":"1.0.0","apiVersion":"^1.0.0"}"#,
            )],
            "不得为 builtin",
        ),
        (
            "api-too-new",
            vec![(
                "mod.json",
                r#"{"id":"api-too-new","version":"1.0.0","apiVersion":"^2.0.0"}"#,
            )],
            "不兼容",
        ),
        (
            "short-version",
            vec![(
                "mod.json",
                r#"{"id":"short-version","version":"1.0","apiVersion":"^1.0.0"}"#,
            )],
            "三段数字",
        ),
        (
            "escape-entry",
            vec![(
                "mod.json",
                r#"{"id":"escape-entry","version":"1.0.0","apiVersion":"^1.0.0","entry":"../x.js"}"#,
            )],
            "entry 路径",
        ),
        (
            "text-entry",
            vec![(
                "mod.json",
                r#"{"id":"text-entry","version":"1.0.0","apiVersion":"^1.0.0","entry":"server.txt"}"#,
            )],
            "entry 路径",
        ),
        (
            "missing-entry",
            vec![(
                "mod.json",
                r#"{"id":"missing-entry","version":"1.0.0","apiVersion":"^1.0.0","entry":"nope.js"}"#,
            )],
            "声明的模块缺失",
        ),
    ];
    for (name, files, needle) in cases {
        write_pkg(&root, name, &files);
        let error = register_root_error(&root);
        assert!(
            error.contains(needle),
            "包 {name} 的错误应包含 {needle:?}，实得 {error}"
        );
        let _ = std::fs::remove_dir_all(root.join(name));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dependency_failures() {
    let root = temp_root("graph");
    write_pkg(
        &root,
        "needs-ghost",
        &[(
            "mod.json",
            r#"{"id":"needs-ghost","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"ghost":"^1.0.0"}}"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(error.contains("依赖的包不存在：ghost"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_root("graph-range");
    write_pkg(
        &root,
        "needs-newer-base",
        &[(
            "mod.json",
            r#"{"id":"needs-newer-base","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"demo-base":"^2.0.0"}}"#,
        )],
    );
    std::fs::create_dir_all(root.join("demo-base")).expect("建依赖目录");
    std::fs::write(
        root.join("demo-base/mod.json"),
        r#"{"id":"demo-base","version":"1.1.0","apiVersion":"^1.0.0"}"#,
    )
    .expect("写依赖 manifest");
    std::fs::write(
        root.join("demo-base/server.js"),
        "export default function register() {}",
    )
    .expect("写依赖入口");
    let error = register_root_error(&root);
    assert!(error.contains("需要 ^2.0.0，实得 1.1.0"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_root("graph-cycle");
    for (name, dependency) in [("cycle-a", "cycle-b"), ("cycle-b", "cycle-a")] {
        write_pkg(
            &root,
            name,
            &[(
                "mod.json",
                format!(
                    r#"{{"id":"{name}","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{{"{dependency}":"^1.0.0"}}}}"#
                )
                .as_str(),
            )],
        );
    }
    let error = register_root_error(&root);
    assert!(error.contains("依赖环"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cross_package_import_rejected() {
    let root = temp_root("cross");
    write_pkg(
        &root,
        "importer",
        &[(
            "server.js",
            r#"
import { helper } from '../provider/helper.js';
export default function register() {}
"#,
        )],
    );
    write_pkg(
        &root,
        "provider",
        &[("helper.js", "export function helper() {}\n")],
    );
    let error = register_root_error(&root);
    assert!(error.contains("跨包导入被拒绝"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn duplicate_package_id_rejected() {
    let root = temp_root("dup-id");
    write_pkg(
        &root,
        "first",
        &[(
            "mod.json",
            r#"{"id":"same-id","version":"1.0.0","apiVersion":"^1.0.0"}"#,
        )],
    );
    write_pkg(
        &root,
        "second",
        &[(
            "mod.json",
            r#"{"id":"same-id","version":"1.0.0","apiVersion":"^1.0.0"}"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(error.contains("重复的包 id"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn symlink_rejected() {
    let root = temp_root("symlink");
    let outside = root.join("outside.js");
    std::fs::write(&outside, "export const x = 1;\n").expect("写包外文件");
    write_pkg(&root, "with-link", &[]);
    std::os::unix::fs::symlink(outside, root.join("with-link").join("linked.js"))
        .expect("建符号链接");
    let error = register_root_error(&root);
    assert!(error.contains("符号链接"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn package_digests_track_server_bytes_only_for_code() {
    let packages = discover_packages(&fixtures_root()).expect("夹具可装载");
    let counter: &ModPackage = packages
        .values()
        .find(|p| p.id() == "demo-counter")
        .expect("counter 夹具存在");
    let mut modified = counter.clone();
    modified.files.insert(
        Path::new("README.md").to_path_buf(),
        b"docs change".to_vec(),
    );
    assert_ne!(
        modified.code_digest(),
        counter.code_digest(),
        "服务端文件变化必须改变代码摘要"
    );
    let mut manifest_changed = counter.clone();
    manifest_changed.files.insert(
        Path::new("mod.json").to_path_buf(),
        br#"{"id":"demo-counter","version":"0.3.1","apiVersion":"^1.0.0","dependencies":{"demo-base":"^2.0.0"},"player":"player.js"}"#
            .to_vec(),
    );
    assert_ne!(
        manifest_changed.code_digest(),
        counter.code_digest(),
        "manifest 字节属于包身份，改动必须改变代码摘要"
    );
    let mut player_changed = counter.clone();
    player_changed.files.insert(
        Path::new("player.js").to_path_buf(),
        b"export default function install(v) {}".to_vec(),
    );
    assert_eq!(
        player_changed.code_digest(),
        counter.code_digest(),
        "玩家模块不参与服务端代码摘要"
    );
    assert_ne!(
        player_changed.player_api_digest(),
        counter.player_api_digest(),
        "玩家模块变化改变玩家面摘要"
    );
}

#[test]
fn dynamic_import_rejected() {
    let root = temp_root("dyn-import");
    write_pkg(
        &root,
        "dyn",
        &[
            (
                "server.js",
                r#"
await import('./other.js');
export default function register() {}
"#,
            ),
            ("other.js", "export const x = 1;\n"),
        ],
    );
    let error = register_root_error(&root);
    assert!(error.contains("不支持动态导入"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dependency_order_is_not_lexicographic() {
    let root = temp_root("topo");
    write_pkg(
        &root,
        "aaa",
        &[(
            "mod.json",
            r#"{"id":"aaa","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"zzz":"^1.0.0"}}"#,
        )],
    );
    write_pkg(&root, "zzz", &[]);
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let order: Vec<&str> = registered
        .contributions
        .iter()
        .map(|c| c.source.namespace.as_str())
        .collect();
    assert_eq!(
        order,
        vec!["zzz", "aaa"],
        "拓扑序由依赖边决定，不受字典序影响"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn module_path_negative_shapes() {
    let root = temp_root("path-shapes");
    let cases: Vec<(&str, &str, &str)> = vec![
        ("abs-entry", r#","entry":"/x.js""#, "entry 路径"),
        ("drive-entry", r#","entry":"C:/x.js""#, "entry 路径"),
        ("backslash-entry", r#","entry":"a\\b.js""#, "entry 路径"),
        ("dot-entry", r#","entry":"./x.js""#, "entry 路径"),
        ("player-escape", r#","player":"../up.js""#, "player 路径"),
        ("player-text", r#","player":"notes.txt""#, "player 路径"),
        (
            "player-equals-entry",
            r#","player":"server.js""#,
            "不得指向同一文件",
        ),
    ];
    for (name, fragment, needle) in cases {
        let manifest =
            format!(r#"{{"id":"{name}","version":"1.0.0","apiVersion":"^1.0.0"{fragment}}}"#);
        write_pkg(&root, name, &[("mod.json", manifest.as_str())]);
        let error = register_root_error(&root);
        assert!(
            error.contains(needle),
            "包 {name} 的错误应包含 {needle:?}，实得 {error}"
        );
        let _ = std::fs::remove_dir_all(root.join(name));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn oversized_file_rejected_before_full_read() {
    let root = temp_root("oversize");
    write_pkg(&root, "big", &[]);
    let big = root.join("big/blob.bin");
    let chunk = vec![0u8; 1024 * 1024];
    let mut file = std::fs::File::create(&big).expect("建大文件");
    for _ in 0..9 {
        std::io::Write::write_all(&mut file, &chunk).expect("写大文件");
    }
    let error = register_root_error(&root);
    assert!(error.contains("超过装载上限"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn symlinked_package_directory_rejected() {
    let root = temp_root("dir-link");
    let real = root.join("real-pkg");
    std::fs::create_dir_all(&real).expect("建真实目录");
    std::fs::write(
        real.join("mod.json"),
        r#"{"id":"real-pkg","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )
    .expect("写 manifest");
    std::fs::write(
        real.join("server.js"),
        "export default function register() {}",
    )
    .expect("写入口");
    std::os::unix::fs::symlink(&real, root.join("linked")).expect("建目录链接");
    let error = register_root_error(&root);
    assert!(error.contains("包目录不得为符号链接"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sdk_internal_modules_are_not_a_public_import_surface() {
    for target in [
        "kkmods:sdk/state.js",
        "kkmods:sdk/registration.js",
        "kkmods:sdk/chains.js",
        "@kkscreeps/mod/state.js",
    ] {
        let root = temp_root("sdk-private");
        let source =
            format!("import {{ bindings }} from '{target}';export default function register(){{}}");
        write_pkg(&root, "internal", &[("server.js", &source)]);
        assert!(
            register_root_error(&root).contains("不支持的外部导入"),
            "{target}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
