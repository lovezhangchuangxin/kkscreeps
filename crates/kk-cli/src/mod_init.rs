//! `mod init`：为作者生成一个可独立运行的最小公共 SDK 包骨架。
//!
//! 模板走真实注册与结算路径（对象 tick、玩家类绑定、场景断言与保存
//! 恢复命令全部可用）；不覆盖既有目录，生成的样例不依赖内部 Rust 代码。
//! 玩家面全局名从包 ID 派生，同一 Mod 根下的多个模板包不互相冲突。

use anyhow::Context;
use std::path::PathBuf;

pub struct Options {
    /// 包根目录；包落在 `<root>/<id>/`，场景的 mods.root 指向本根。
    pub root: PathBuf,
    /// 包 ID，同时是子目录名与命名空间前缀。
    pub id: String,
}

pub fn run(options: Options) -> i32 {
    match init(&options) {
        Ok(path) => {
            println!("mod init：已生成 {}/ 下的模板包与场景", path.display());
            println!(
                "检查组合：  kkscreeps mod inspect --root {} --report /tmp/{}-inspect.json",
                options.root.display(),
                options.id
            );
            println!(
                "运行场景：  kkscreeps sim run --scenario {}/scenario.json --ticks 3 --report /tmp/{}.json",
                path.display(),
                options.id
            );
            println!(
                "保存恢复：  同命令加 --save-checkpoint /tmp/{}-cp.json（首拍）后接 --load-checkpoint 续跑",
                options.id
            );
            0
        }
        Err(error) => {
            eprintln!("mod init: {error:#}");
            1
        }
    }
}

fn init(options: &Options) -> anyhow::Result<PathBuf> {
    let id = &options.id;
    anyhow::ensure!(
        !id.contains('/') && !id.contains('\\'),
        "包 ID 不能包含路径分隔符"
    );
    // 包 ID 是命名空间前缀；借探针键复用 Key 的命名空间校验规则
    // （以小写字母开头、小写字母/数字/_/-、不超过 64 字节）。
    kk_mod_api::Key::new(format!("{id}:probe")).map_err(|_| {
        anyhow::anyhow!("包 ID 须以小写字母开头，仅含小写字母/数字/_/- 且不超过 64 字节")
    })?;
    let package_dir = options.root.join(id);
    anyhow::ensure!(
        !package_dir.exists(),
        "目标 {} 已存在；mod init 不覆盖现有目录",
        package_dir.display()
    );
    std::fs::create_dir_all(&package_dir)?;
    let names = Names::of(id);
    let files: [(&str, String); 6] = [
        ("mod.json", manifest(id)),
        ("server.js", server(id, &names)),
        ("player.js", player(id, &names)),
        // 玩家模块目录即场景目录；bots.modules="." 解析到这里的 main.js。
        ("main.js", main_js(&names)),
        ("scenario.json", scenario(id)),
        ("README.md", readme(id, &names)),
    ];
    for (name, body) in files {
        std::fs::write(package_dir.join(name), body)
            .with_context(|| format!("写入 {name} 失败"))?;
    }
    Ok(package_dir)
}

/// 从包 ID 派生的玩家面全局名；ID 字符集（小写字母/数字/_/-）可安全
/// 转换，保证同一 Mod 根下多个模板包的玩家类与查询常量互不冲突。
struct Names {
    class: String,
    find: String,
    look: String,
}

impl Names {
    fn of(id: &str) -> Self {
        let upper = id.replace(['-', '_'], "_").to_ascii_uppercase();
        let class = id
            .split(['-', '_'])
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<String>()
            + "Counter";
        Self {
            class,
            find: format!("FIND_{upper}_COUNTERS"),
            look: format!("LOOK_{upper}_COUNTERS"),
        }
    }
}

fn manifest(id: &str) -> String {
    format!(
        r#"{{
  "id": "{id}",
  "version": "1.0.0",
  "apiVersion": "^1.0.0",
  "entry": "server.js",
  "player": "player.js"
}}
"#
    )
}

fn server(id: &str, names: &Names) -> String {
    format!(
        r#"// {id} 的服务端声明：一个每 tick 自增的计数对象。
// 宿主在对象 tick 阶段按对象 ID 升序调用 tick；ctx.state 是本包的持久
// namespace state，随检查点保存并在冷启动后延续（整值替换语义）。
export default function register(api) {{
  api.objects.define('{id}:counter', {{
    version: 1,
    playerClass: '{class}',
    visibility: 'room-visible',
    positionFields: ['x', 'y'],
    fields: {{
      x: {{ type: 'u32', min: 0, max: 49 }},
      y: {{ type: 'u32', min: 0, max: 49 }},
      value: {{ type: 'u32', default: 0 }},
    }},
    tick(ctx) {{
      ctx.target.fields.value += 1;
      ctx.state = {{ ticks: (ctx.state?.ticks ?? 0) + 1 }};
    }},
  }});
}}
"#,
        class = names.class
    )
}

fn player(id: &str, names: &Names) -> String {
    format!(
        r#"// {id} 的玩家面：绑定对象类并安装 FIND/LOOK 查询常量。
export default function install(api) {{
  class {class} extends api.builtins.RoomObject {{
    get value() {{ return api.view.field(this, 'value'); }}
  }}
  api.objects.bind('{id}:counter', {class});
  api.globals.define('{class}', {class});
  const queries = api.queries.install('{id}:counter');
  api.globals.define('{find}', queries.find);
  api.globals.define('{look}', queries.look);
}}
"#,
        class = names.class,
        find = names.find,
        look = names.look
    )
}

fn main_js(names: &Names) -> String {
    format!(
        r#"// 玩家脚本：读取计数对象当前值写入 Memory，供场景断言观察。
// 玩家每轮读到的是上一轮结算后的已提交值，因此末轮读到 value-1。
module.exports.loop = function () {{
  const room = Game.rooms.W1N1;
  const counters = room ? room.find({find}) : [];
  Memory.count = counters.length ? counters[0].value : null;
}};
"#,
        find = names.find
    )
}

fn scenario(id: &str) -> String {
    r#"{
  "version": 1,
  "mods": { "root": ".." },
  "shard": {
    "players": ["p1"],
    "rooms": [{ "name": "W1N1", "owner": "p1" }]
  },
  "bots": [{ "player": "p1", "modules": "." }],
  "objects": [
    {
      "alias": "ctr",
      "room": "W1N1",
      "kind": "mod",
      "spec": { "typeKey": "__ID__:counter", "fields": { "x": 10, "y": 10 } }
    }
  ],
  "assertions": [
    { "pointer": "/objects/ctr/fields/value", "op": "eq", "value": 3 },
    { "pointer": "/players/p1/memory/count", "op": "eq", "value": 2 },
    { "pointer": "/mod_states/__ID__/ticks", "op": "eq", "value": 3 }
  ]
}
"#
    .replace("__ID__", id)
}

fn readme(id: &str, names: &Names) -> String {
    format!(
        r#"# {id}（mod init 模板）

一个最小的公共 SDK 包：server.js 声明每 tick 自增的计数对象，player.js
绑定 {class} 类并安装 {find} 查询，main.js 是玩家脚本，scenario.json
引用本包并断言 3 tick 后的计数。宿主没有按包 ID 分派的 Rust 分支。

在生成时传给 `mod init --root` 的目录（本包的上级目录，其下应只含包
目录）执行以下命令；kkscreeps 指向已构建的 kkscreeps 仓库二进制
（如 `<仓库>/target/release/kkscreeps`）：

```bash
# 1) 检查冻结组合与生效链
kkscreeps mod inspect --root . --report /tmp/{id}-inspect.json

# 2) 运行场景（--ticks 至少 3 才满足断言）
kkscreeps sim run --scenario {id}/scenario.json --ticks 3 --report /tmp/{id}.json

# 3) 保存恢复：先跑 1 轮保存（断言未满足时退出码 2 是预期），再加载续跑
kkscreeps sim run --scenario {id}/scenario.json --ticks 1 --save-checkpoint /tmp/{id}-cp.json --report /tmp/{id}-p1.json; echo $?
kkscreeps sim run --scenario {id}/scenario.json --ticks 2 --load-checkpoint /tmp/{id}-cp.json --report /tmp/{id}-p2.json
```

断言中对象 `value` 为 3 而玩家 `memory.count` 为 2：玩家每轮读到的是
上一轮结算后的已提交值。对象 tick 与 `ctx.state` 的契约见 kkscreeps
仓库 docs/architecture/mod-object-tick.md；场景与断言语法见同目录
local-simulation.md。

本包目录可改名（装载按 manifest id 识别）。复制派生新包时须同步修改
mod.json 的 id 以及 server.js/player.js/scenario.json 中全部 `{id}:`
前缀；同一 Mod 根下出现重复 id 会被拒绝。scenario.json 的 `mods.root`
指向上级目录（包发现按子目录进行），移动目录时须同步调整。
"#,
        class = names.class,
        find = names.find
    )
}
