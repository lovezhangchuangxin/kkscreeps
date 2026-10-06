//! 固定上游的最小地形适配；锚点变化时拒绝构建，避免静默修改算法。
use std::path::Path;

fn replace_once(source: &mut String, anchor: &str, replacement: &str) {
    assert_eq!(
        source.matches(anchor).count(),
        1,
        "上游地形适配锚点不唯一：{anchor}"
    );
    *source = source.replacen(anchor, replacement, 1);
}

pub fn generate(out: &Path) {
    let mut header = std::fs::read_to_string("native/pf.h").expect("读取固定上游头文件");
    let mut source = std::fs::read_to_string("native/pf.cc").expect("读取固定上游算法");
    replace_once(
        &mut header,
        "static std::array<uint8_t*, map_position_size> terrain;",
        "const uint8_t* (*terrain_query)(void*, uint8_t, uint8_t) = nullptr;\n\t\t\tvoid* terrain_user = nullptr;",
    );
    replace_once(
        &mut header,
        "static void load_terrain(v8::Local<v8::Array> terrain);",
        "void bind_terrain(const uint8_t* (*query)(void*, uint8_t, uint8_t), void* user) { terrain_query = query; terrain_user = user; }\n\t\t\tvoid release_search() { _is_in_use = false; room_callback = nullptr; room_data_handles = nullptr; bind_terrain(nullptr, nullptr); for (size_t ii = 0; ii < room_table_size; ++ii) { room_table[ii].terrain = nullptr; room_table[ii].cost_matrix = nullptr; } }",
    );
    replace_once(
        &mut source,
        "\tdecltype(path_finder_t::terrain) path_finder_t::terrain = {{ nullptr }};",
        "",
    );
    replace_once(
        &mut source,
        "uint8_t* terrain_ptr = terrain[map_pos.id];",
        "uint8_t* terrain_ptr = const_cast<uint8_t*>(terrain_query ? terrain_query(terrain_user, map_pos.xx, map_pos.yy) : nullptr);",
    );
    let marker = "\t// Loads static terrain data into module upfront";
    assert_eq!(source.matches(marker).count(), 1, "上游地形载入锚点不唯一");
    let tail = source.split_off(source.find(marker).unwrap());
    assert!(tail.starts_with(
        "\t// Loads static terrain data into module upfront\n\tvoid path_finder_t::load_terrain"
    ));
    // 固定函数完整尾段与原文核对，末尾变动也须显式更新适配。
    assert_eq!(tail, include_str!("native/terrain_load_anchor.txt"));
    std::fs::write(out.join("pf.h"), header).expect("写适配头文件");
    std::fs::write(out.join("pf.cc"), source).expect("写适配算法");
}
