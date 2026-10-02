//! 由 JSON 常量数据生成的 Rust 常量表。
//!
//! 数据来源：`crates/kk-runner/assets/constants.json`，来自上游 `common/lib/constants.js`。
//! 修改数据或生成器后运行 `node scripts/gen-constants.mjs`，不要直接修改本文件。
//! 常量按输入对象的 JavaScript 枚举顺序生成；复杂值保存为 JSON 字符串，调用方按需解析。
#![allow(clippy::all)]

pub const OK: i64 = 0i64;
pub const ERR_NOT_OWNER: i64 = -1i64;
pub const ERR_NO_PATH: i64 = -2i64;
pub const ERR_NAME_EXISTS: i64 = -3i64;
pub const ERR_BUSY: i64 = -4i64;
pub const ERR_NOT_FOUND: i64 = -5i64;
pub const ERR_NOT_ENOUGH_ENERGY: i64 = -6i64;
pub const ERR_NOT_ENOUGH_RESOURCES: i64 = -6i64;
pub const ERR_INVALID_TARGET: i64 = -7i64;
pub const ERR_FULL: i64 = -8i64;
pub const ERR_NOT_IN_RANGE: i64 = -9i64;
pub const ERR_INVALID_ARGS: i64 = -10i64;
pub const ERR_TIRED: i64 = -11i64;
pub const ERR_NO_BODYPART: i64 = -12i64;
pub const ERR_NOT_ENOUGH_EXTENSIONS: i64 = -6i64;
pub const ERR_RCL_NOT_ENOUGH: i64 = -14i64;
pub const ERR_GCL_NOT_ENOUGH: i64 = -15i64;
pub const ERR_ACCESS_DENIED: i64 = -16i64;
pub const FIND_EXIT_TOP: i64 = 1i64;
pub const FIND_EXIT_RIGHT: i64 = 3i64;
pub const FIND_EXIT_BOTTOM: i64 = 5i64;
pub const FIND_EXIT_LEFT: i64 = 7i64;
pub const FIND_EXIT: i64 = 10i64;
pub const FIND_CREEPS: i64 = 101i64;
pub const FIND_MY_CREEPS: i64 = 102i64;
pub const FIND_HOSTILE_CREEPS: i64 = 103i64;
pub const FIND_SOURCES_ACTIVE: i64 = 104i64;
pub const FIND_SOURCES: i64 = 105i64;
pub const FIND_DROPPED_RESOURCES: i64 = 106i64;
pub const FIND_STRUCTURES: i64 = 107i64;
pub const FIND_MY_STRUCTURES: i64 = 108i64;
pub const FIND_HOSTILE_STRUCTURES: i64 = 109i64;
pub const FIND_FLAGS: i64 = 110i64;
pub const FIND_CONSTRUCTION_SITES: i64 = 111i64;
pub const FIND_MY_SPAWNS: i64 = 112i64;
pub const FIND_HOSTILE_SPAWNS: i64 = 113i64;
pub const FIND_MY_CONSTRUCTION_SITES: i64 = 114i64;
pub const FIND_HOSTILE_CONSTRUCTION_SITES: i64 = 115i64;
pub const FIND_MINERALS: i64 = 116i64;
pub const FIND_NUKES: i64 = 117i64;
pub const FIND_TOMBSTONES: i64 = 118i64;
pub const FIND_POWER_CREEPS: i64 = 119i64;
pub const FIND_MY_POWER_CREEPS: i64 = 120i64;
pub const FIND_HOSTILE_POWER_CREEPS: i64 = 121i64;
pub const FIND_DEPOSITS: i64 = 122i64;
pub const FIND_RUINS: i64 = 123i64;
pub const TOP: i64 = 1i64;
pub const TOP_RIGHT: i64 = 2i64;
pub const RIGHT: i64 = 3i64;
pub const BOTTOM_RIGHT: i64 = 4i64;
pub const BOTTOM: i64 = 5i64;
pub const BOTTOM_LEFT: i64 = 6i64;
pub const LEFT: i64 = 7i64;
pub const TOP_LEFT: i64 = 8i64;
pub const COLOR_RED: i64 = 1i64;
pub const COLOR_PURPLE: i64 = 2i64;
pub const COLOR_BLUE: i64 = 3i64;
pub const COLOR_CYAN: i64 = 4i64;
pub const COLOR_GREEN: i64 = 5i64;
pub const COLOR_YELLOW: i64 = 6i64;
pub const COLOR_ORANGE: i64 = 7i64;
pub const COLOR_BROWN: i64 = 8i64;
pub const COLOR_GREY: i64 = 9i64;
pub const COLOR_WHITE: i64 = 10i64;
pub const LOOK_CREEPS: &str = "creep";
pub const LOOK_ENERGY: &str = "energy";
pub const LOOK_RESOURCES: &str = "resource";
pub const LOOK_SOURCES: &str = "source";
pub const LOOK_MINERALS: &str = "mineral";
pub const LOOK_DEPOSITS: &str = "deposit";
pub const LOOK_STRUCTURES: &str = "structure";
pub const LOOK_FLAGS: &str = "flag";
pub const LOOK_CONSTRUCTION_SITES: &str = "constructionSite";
pub const LOOK_NUKES: &str = "nuke";
pub const LOOK_TERRAIN: &str = "terrain";
pub const LOOK_TOMBSTONES: &str = "tombstone";
pub const LOOK_POWER_CREEPS: &str = "powerCreep";
pub const LOOK_RUINS: &str = "ruin";
pub const OBSTACLE_OBJECT_TYPES: &[&str] = &[
    "spawn",
    "creep",
    "powerCreep",
    "source",
    "mineral",
    "deposit",
    "controller",
    "constructedWall",
    "extension",
    "link",
    "storage",
    "tower",
    "observer",
    "powerSpawn",
    "powerBank",
    "lab",
    "terminal",
    "nuker",
    "factory",
    "invaderCore",
];
pub const MOVE: &str = "move";
pub const WORK: &str = "work";
pub const CARRY: &str = "carry";
pub const ATTACK: &str = "attack";
pub const RANGED_ATTACK: &str = "ranged_attack";
pub const TOUGH: &str = "tough";
pub const HEAL: &str = "heal";
pub const CLAIM: &str = "claim";
pub const BODYPART_COST: &[(&str, i64)] = &[
    ("move", 50i64),
    ("work", 100i64),
    ("attack", 80i64),
    ("carry", 50i64),
    ("heal", 250i64),
    ("ranged_attack", 150i64),
    ("tough", 10i64),
    ("claim", 600i64),
];
pub const WORLD_WIDTH: i64 = 202i64;
pub const WORLD_HEIGHT: i64 = 202i64;
pub const CREEP_LIFE_TIME: i64 = 1500i64;
pub const CREEP_CLAIM_LIFE_TIME: i64 = 600i64;
pub const CREEP_CORPSE_RATE: f64 = 0.2_f64;
pub const CREEP_PART_MAX_ENERGY: i64 = 125i64;
pub const CARRY_CAPACITY: i64 = 50i64;
pub const HARVEST_POWER: i64 = 2i64;
pub const HARVEST_MINERAL_POWER: i64 = 1i64;
pub const HARVEST_DEPOSIT_POWER: i64 = 1i64;
pub const REPAIR_POWER: i64 = 100i64;
pub const DISMANTLE_POWER: i64 = 50i64;
pub const BUILD_POWER: i64 = 5i64;
pub const ATTACK_POWER: i64 = 30i64;
pub const UPGRADE_CONTROLLER_POWER: i64 = 1i64;
pub const RANGED_ATTACK_POWER: i64 = 10i64;
pub const HEAL_POWER: i64 = 12i64;
pub const RANGED_HEAL_POWER: i64 = 4i64;
pub const REPAIR_COST: f64 = 0.01_f64;
pub const DISMANTLE_COST: f64 = 0.005_f64;
pub const RAMPART_DECAY_AMOUNT: i64 = 300i64;
pub const RAMPART_DECAY_TIME: i64 = 100i64;
pub const RAMPART_HITS: i64 = 1i64;
pub const RAMPART_HITS_MAX: &[(&str, i64)] = &[
    ("2", 300000i64),
    ("3", 1000000i64),
    ("4", 3000000i64),
    ("5", 10000000i64),
    ("6", 30000000i64),
    ("7", 100000000i64),
    ("8", 300000000i64),
];
pub const ENERGY_REGEN_TIME: i64 = 300i64;
pub const ENERGY_DECAY: i64 = 1000i64;
pub const SPAWN_HITS: i64 = 5000i64;
pub const SPAWN_ENERGY_START: i64 = 300i64;
pub const SPAWN_ENERGY_CAPACITY: i64 = 300i64;
pub const CREEP_SPAWN_TIME: i64 = 3i64;
pub const SPAWN_RENEW_RATIO: f64 = 1.2_f64;
pub const SOURCE_ENERGY_CAPACITY: i64 = 3000i64;
pub const SOURCE_ENERGY_NEUTRAL_CAPACITY: i64 = 1500i64;
pub const SOURCE_ENERGY_KEEPER_CAPACITY: i64 = 4000i64;
pub const WALL_HITS: i64 = 1i64;
pub const WALL_HITS_MAX: i64 = 300000000i64;
pub const EXTENSION_HITS: i64 = 1000i64;
pub const EXTENSION_ENERGY_CAPACITY: &[(&str, i64)] = &[
    ("0", 50i64),
    ("1", 50i64),
    ("2", 50i64),
    ("3", 50i64),
    ("4", 50i64),
    ("5", 50i64),
    ("6", 50i64),
    ("7", 100i64),
    ("8", 200i64),
];
pub const ROAD_HITS: i64 = 5000i64;
pub const ROAD_WEAROUT: i64 = 1i64;
pub const ROAD_WEAROUT_POWER_CREEP: i64 = 100i64;
pub const ROAD_DECAY_AMOUNT: i64 = 100i64;
pub const ROAD_DECAY_TIME: i64 = 1000i64;
pub const LINK_HITS: i64 = 1000i64;
pub const LINK_HITS_MAX: i64 = 1000i64;
pub const LINK_CAPACITY: i64 = 800i64;
pub const LINK_COOLDOWN: i64 = 1i64;
pub const LINK_LOSS_RATIO: f64 = 0.03_f64;
pub const STORAGE_CAPACITY: i64 = 1000000i64;
pub const STORAGE_HITS: i64 = 10000i64;
pub const STRUCTURE_SPAWN: &str = "spawn";
pub const STRUCTURE_EXTENSION: &str = "extension";
pub const STRUCTURE_ROAD: &str = "road";
pub const STRUCTURE_WALL: &str = "constructedWall";
pub const STRUCTURE_RAMPART: &str = "rampart";
pub const STRUCTURE_KEEPER_LAIR: &str = "keeperLair";
pub const STRUCTURE_PORTAL: &str = "portal";
pub const STRUCTURE_CONTROLLER: &str = "controller";
pub const STRUCTURE_LINK: &str = "link";
pub const STRUCTURE_STORAGE: &str = "storage";
pub const STRUCTURE_TOWER: &str = "tower";
pub const STRUCTURE_OBSERVER: &str = "observer";
pub const STRUCTURE_POWER_BANK: &str = "powerBank";
pub const STRUCTURE_POWER_SPAWN: &str = "powerSpawn";
pub const STRUCTURE_EXTRACTOR: &str = "extractor";
pub const STRUCTURE_LAB: &str = "lab";
pub const STRUCTURE_TERMINAL: &str = "terminal";
pub const STRUCTURE_CONTAINER: &str = "container";
pub const STRUCTURE_NUKER: &str = "nuker";
pub const STRUCTURE_FACTORY: &str = "factory";
pub const STRUCTURE_INVADER_CORE: &str = "invaderCore";
pub const CONSTRUCTION_COST: &[(&str, i64)] = &[
    ("spawn", 15000i64),
    ("extension", 3000i64),
    ("road", 300i64),
    ("constructedWall", 1i64),
    ("rampart", 1i64),
    ("link", 5000i64),
    ("storage", 30000i64),
    ("tower", 5000i64),
    ("observer", 8000i64),
    ("powerSpawn", 100000i64),
    ("extractor", 5000i64),
    ("lab", 50000i64),
    ("terminal", 100000i64),
    ("container", 5000i64),
    ("nuker", 100000i64),
    ("factory", 100000i64),
];
pub const CONSTRUCTION_COST_ROAD_SWAMP_RATIO: i64 = 5i64;
pub const CONSTRUCTION_COST_ROAD_WALL_RATIO: i64 = 150i64;
pub const CONTROLLER_LEVELS: &[(&str, i64)] = &[
    ("1", 200i64),
    ("2", 45000i64),
    ("3", 135000i64),
    ("4", 405000i64),
    ("5", 1215000i64),
    ("6", 3645000i64),
    ("7", 10935000i64),
];
pub const CONTROLLER_STRUCTURES: &str = r##"{"spawn":{"0":0,"1":1,"2":1,"3":1,"4":1,"5":1,"6":1,"7":2,"8":3},"extension":{"0":0,"1":0,"2":5,"3":10,"4":20,"5":30,"6":40,"7":50,"8":60},"link":{"1":0,"2":0,"3":0,"4":0,"5":2,"6":3,"7":4,"8":6},"road":{"0":2500,"1":2500,"2":2500,"3":2500,"4":2500,"5":2500,"6":2500,"7":2500,"8":2500},"constructedWall":{"1":0,"2":2500,"3":2500,"4":2500,"5":2500,"6":2500,"7":2500,"8":2500},"rampart":{"1":0,"2":2500,"3":2500,"4":2500,"5":2500,"6":2500,"7":2500,"8":2500},"storage":{"1":0,"2":0,"3":0,"4":1,"5":1,"6":1,"7":1,"8":1},"tower":{"1":0,"2":0,"3":1,"4":1,"5":2,"6":2,"7":3,"8":6},"observer":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":0,"7":0,"8":1},"powerSpawn":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":0,"7":0,"8":1},"extractor":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":1,"7":1,"8":1},"terminal":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":1,"7":1,"8":1},"lab":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":3,"7":6,"8":10},"container":{"0":5,"1":5,"2":5,"3":5,"4":5,"5":5,"6":5,"7":5,"8":5},"nuker":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":0,"7":0,"8":1},"factory":{"1":0,"2":0,"3":0,"4":0,"5":0,"6":0,"7":1,"8":1}}"##;
pub const CONTROLLER_DOWNGRADE: &[(&str, i64)] = &[
    ("1", 20000i64),
    ("2", 10000i64),
    ("3", 20000i64),
    ("4", 40000i64),
    ("5", 80000i64),
    ("6", 120000i64),
    ("7", 150000i64),
    ("8", 200000i64),
];
pub const CONTROLLER_DOWNGRADE_RESTORE: i64 = 100i64;
pub const CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD: i64 = 5000i64;
pub const CONTROLLER_CLAIM_DOWNGRADE: i64 = 300i64;
pub const CONTROLLER_RESERVE: i64 = 1i64;
pub const CONTROLLER_RESERVE_MAX: i64 = 5000i64;
pub const CONTROLLER_MAX_UPGRADE_PER_TICK: i64 = 15i64;
pub const CONTROLLER_ATTACK_BLOCKED_UPGRADE: i64 = 1000i64;
pub const CONTROLLER_NUKE_BLOCKED_UPGRADE: i64 = 200i64;
pub const SAFE_MODE_DURATION: i64 = 20000i64;
pub const SAFE_MODE_COOLDOWN: i64 = 50000i64;
pub const SAFE_MODE_COST: i64 = 1000i64;
pub const TOWER_HITS: i64 = 3000i64;
pub const TOWER_CAPACITY: i64 = 1000i64;
pub const TOWER_ENERGY_COST: i64 = 10i64;
pub const TOWER_POWER_ATTACK: i64 = 600i64;
pub const TOWER_POWER_HEAL: i64 = 400i64;
pub const TOWER_POWER_REPAIR: i64 = 800i64;
pub const TOWER_OPTIMAL_RANGE: i64 = 5i64;
pub const TOWER_FALLOFF_RANGE: i64 = 20i64;
pub const TOWER_FALLOFF: f64 = 0.75_f64;
pub const OBSERVER_HITS: i64 = 500i64;
pub const OBSERVER_RANGE: i64 = 10i64;
pub const POWER_BANK_HITS: i64 = 2000000i64;
pub const POWER_BANK_CAPACITY_MAX: i64 = 5000i64;
pub const POWER_BANK_CAPACITY_MIN: i64 = 500i64;
pub const POWER_BANK_CAPACITY_CRIT: f64 = 0.3_f64;
pub const POWER_BANK_DECAY: i64 = 5000i64;
pub const POWER_BANK_HIT_BACK: f64 = 0.5_f64;
pub const POWER_SPAWN_HITS: i64 = 5000i64;
pub const POWER_SPAWN_ENERGY_CAPACITY: i64 = 5000i64;
pub const POWER_SPAWN_POWER_CAPACITY: i64 = 100i64;
pub const POWER_SPAWN_ENERGY_RATIO: i64 = 50i64;
pub const EXTRACTOR_HITS: i64 = 500i64;
pub const EXTRACTOR_COOLDOWN: i64 = 5i64;
pub const LAB_HITS: i64 = 500i64;
pub const LAB_MINERAL_CAPACITY: i64 = 3000i64;
pub const LAB_ENERGY_CAPACITY: i64 = 2000i64;
pub const LAB_BOOST_ENERGY: i64 = 20i64;
pub const LAB_BOOST_MINERAL: i64 = 30i64;
pub const LAB_COOLDOWN: i64 = 10i64;
pub const LAB_REACTION_AMOUNT: i64 = 5i64;
pub const LAB_UNBOOST_ENERGY: i64 = 0i64;
pub const LAB_UNBOOST_MINERAL: i64 = 15i64;
pub const GCL_POW: f64 = 2.4_f64;
pub const GCL_MULTIPLY: i64 = 1000000i64;
pub const GCL_NOVICE: i64 = 3i64;
pub const MODE_SIMULATION: &str = r##"null"##;
pub const MODE_WORLD: &str = r##"null"##;
pub const TERRAIN_MASK_WALL: i64 = 1i64;
pub const TERRAIN_MASK_SWAMP: i64 = 2i64;
pub const TERRAIN_MASK_LAVA: i64 = 4i64;
pub const MAX_CONSTRUCTION_SITES: i64 = 100i64;
pub const MAX_CREEP_SIZE: i64 = 50i64;
pub const MINERAL_REGEN_TIME: i64 = 50000i64;
pub const MINERAL_MIN_AMOUNT: &[(&str, i64)] = &[
    ("H", 35000i64),
    ("O", 35000i64),
    ("L", 35000i64),
    ("K", 35000i64),
    ("Z", 35000i64),
    ("U", 35000i64),
    ("X", 35000i64),
];
pub const MINERAL_RANDOM_FACTOR: i64 = 2i64;
pub const MINERAL_DENSITY: &[(&str, i64)] = &[
    ("1", 15000i64),
    ("2", 35000i64),
    ("3", 70000i64),
    ("4", 100000i64),
];
pub const MINERAL_DENSITY_PROBABILITY: &[(&str, f64)] =
    &[("1", 0.1), ("2", 0.5), ("3", 0.9), ("4", 1.0_f64)];
pub const MINERAL_DENSITY_CHANGE: f64 = 0.05_f64;
pub const DENSITY_LOW: i64 = 1i64;
pub const DENSITY_MODERATE: i64 = 2i64;
pub const DENSITY_HIGH: i64 = 3i64;
pub const DENSITY_ULTRA: i64 = 4i64;
pub const DEPOSIT_EXHAUST_MULTIPLY: f64 = 0.001_f64;
pub const DEPOSIT_EXHAUST_POW: f64 = 1.2_f64;
pub const DEPOSIT_DECAY_TIME: i64 = 50000i64;
pub const TERMINAL_CAPACITY: i64 = 300000i64;
pub const TERMINAL_HITS: i64 = 3000i64;
pub const TERMINAL_SEND_COST: f64 = 0.1_f64;
pub const TERMINAL_MIN_SEND: i64 = 100i64;
pub const TERMINAL_COOLDOWN: i64 = 10i64;
pub const CONTAINER_HITS: i64 = 250000i64;
pub const CONTAINER_CAPACITY: i64 = 2000i64;
pub const CONTAINER_DECAY: i64 = 5000i64;
pub const CONTAINER_DECAY_TIME: i64 = 100i64;
pub const CONTAINER_DECAY_TIME_OWNED: i64 = 500i64;
pub const NUKER_HITS: i64 = 1000i64;
pub const NUKER_COOLDOWN: i64 = 100000i64;
pub const NUKER_ENERGY_CAPACITY: i64 = 300000i64;
pub const NUKER_GHODIUM_CAPACITY: i64 = 5000i64;
pub const NUKE_LAND_TIME: i64 = 50000i64;
pub const NUKE_RANGE: i64 = 10i64;
pub const NUKE_DAMAGE: &[(&str, i64)] = &[("0", 10000000i64), ("2", 5000000i64)];
pub const FACTORY_HITS: i64 = 1000i64;
pub const FACTORY_CAPACITY: i64 = 50000i64;
pub const TOMBSTONE_DECAY_PER_PART: i64 = 5i64;
pub const TOMBSTONE_DECAY_POWER_CREEP: i64 = 500i64;
pub const RUIN_DECAY: i64 = 500i64;
pub const RUIN_DECAY_STRUCTURES: &[(&str, i64)] = &[("powerBank", 10i64)];
pub const PORTAL_DECAY: i64 = 30000i64;
pub const ORDER_SELL: &str = "sell";
pub const ORDER_BUY: &str = "buy";
pub const MARKET_FEE: f64 = 0.05_f64;
pub const MARKET_MAX_ORDERS: i64 = 300i64;
pub const MARKET_ORDER_LIFE_TIME: i64 = 2592000000i64;
pub const FLAGS_LIMIT: i64 = 10000i64;
pub const SUBSCRIPTION_TOKEN: &str = "token";
pub const CPU_UNLOCK: &str = "cpuUnlock";
pub const PIXEL: &str = "pixel";
pub const ACCESS_KEY: &str = "accessKey";
pub const PIXEL_CPU_COST: i64 = 10000i64;
pub const RESOURCE_ENERGY: &str = "energy";
pub const RESOURCE_POWER: &str = "power";
pub const RESOURCE_HYDROGEN: &str = "H";
pub const RESOURCE_OXYGEN: &str = "O";
pub const RESOURCE_UTRIUM: &str = "U";
pub const RESOURCE_LEMERGIUM: &str = "L";
pub const RESOURCE_KEANIUM: &str = "K";
pub const RESOURCE_ZYNTHIUM: &str = "Z";
pub const RESOURCE_CATALYST: &str = "X";
pub const RESOURCE_GHODIUM: &str = "G";
pub const RESOURCE_SILICON: &str = "silicon";
pub const RESOURCE_METAL: &str = "metal";
pub const RESOURCE_BIOMASS: &str = "biomass";
pub const RESOURCE_MIST: &str = "mist";
pub const RESOURCE_HYDROXIDE: &str = "OH";
pub const RESOURCE_ZYNTHIUM_KEANITE: &str = "ZK";
pub const RESOURCE_UTRIUM_LEMERGITE: &str = "UL";
pub const RESOURCE_UTRIUM_HYDRIDE: &str = "UH";
pub const RESOURCE_UTRIUM_OXIDE: &str = "UO";
pub const RESOURCE_KEANIUM_HYDRIDE: &str = "KH";
pub const RESOURCE_KEANIUM_OXIDE: &str = "KO";
pub const RESOURCE_LEMERGIUM_HYDRIDE: &str = "LH";
pub const RESOURCE_LEMERGIUM_OXIDE: &str = "LO";
pub const RESOURCE_ZYNTHIUM_HYDRIDE: &str = "ZH";
pub const RESOURCE_ZYNTHIUM_OXIDE: &str = "ZO";
pub const RESOURCE_GHODIUM_HYDRIDE: &str = "GH";
pub const RESOURCE_GHODIUM_OXIDE: &str = "GO";
pub const RESOURCE_UTRIUM_ACID: &str = "UH2O";
pub const RESOURCE_UTRIUM_ALKALIDE: &str = "UHO2";
pub const RESOURCE_KEANIUM_ACID: &str = "KH2O";
pub const RESOURCE_KEANIUM_ALKALIDE: &str = "KHO2";
pub const RESOURCE_LEMERGIUM_ACID: &str = "LH2O";
pub const RESOURCE_LEMERGIUM_ALKALIDE: &str = "LHO2";
pub const RESOURCE_ZYNTHIUM_ACID: &str = "ZH2O";
pub const RESOURCE_ZYNTHIUM_ALKALIDE: &str = "ZHO2";
pub const RESOURCE_GHODIUM_ACID: &str = "GH2O";
pub const RESOURCE_GHODIUM_ALKALIDE: &str = "GHO2";
pub const RESOURCE_CATALYZED_UTRIUM_ACID: &str = "XUH2O";
pub const RESOURCE_CATALYZED_UTRIUM_ALKALIDE: &str = "XUHO2";
pub const RESOURCE_CATALYZED_KEANIUM_ACID: &str = "XKH2O";
pub const RESOURCE_CATALYZED_KEANIUM_ALKALIDE: &str = "XKHO2";
pub const RESOURCE_CATALYZED_LEMERGIUM_ACID: &str = "XLH2O";
pub const RESOURCE_CATALYZED_LEMERGIUM_ALKALIDE: &str = "XLHO2";
pub const RESOURCE_CATALYZED_ZYNTHIUM_ACID: &str = "XZH2O";
pub const RESOURCE_CATALYZED_ZYNTHIUM_ALKALIDE: &str = "XZHO2";
pub const RESOURCE_CATALYZED_GHODIUM_ACID: &str = "XGH2O";
pub const RESOURCE_CATALYZED_GHODIUM_ALKALIDE: &str = "XGHO2";
pub const RESOURCE_OPS: &str = "ops";
pub const RESOURCE_UTRIUM_BAR: &str = "utrium_bar";
pub const RESOURCE_LEMERGIUM_BAR: &str = "lemergium_bar";
pub const RESOURCE_ZYNTHIUM_BAR: &str = "zynthium_bar";
pub const RESOURCE_KEANIUM_BAR: &str = "keanium_bar";
pub const RESOURCE_GHODIUM_MELT: &str = "ghodium_melt";
pub const RESOURCE_OXIDANT: &str = "oxidant";
pub const RESOURCE_REDUCTANT: &str = "reductant";
pub const RESOURCE_PURIFIER: &str = "purifier";
pub const RESOURCE_BATTERY: &str = "battery";
pub const RESOURCE_COMPOSITE: &str = "composite";
pub const RESOURCE_CRYSTAL: &str = "crystal";
pub const RESOURCE_LIQUID: &str = "liquid";
pub const RESOURCE_WIRE: &str = "wire";
pub const RESOURCE_SWITCH: &str = "switch";
pub const RESOURCE_TRANSISTOR: &str = "transistor";
pub const RESOURCE_MICROCHIP: &str = "microchip";
pub const RESOURCE_CIRCUIT: &str = "circuit";
pub const RESOURCE_DEVICE: &str = "device";
pub const RESOURCE_CELL: &str = "cell";
pub const RESOURCE_PHLEGM: &str = "phlegm";
pub const RESOURCE_TISSUE: &str = "tissue";
pub const RESOURCE_MUSCLE: &str = "muscle";
pub const RESOURCE_ORGANOID: &str = "organoid";
pub const RESOURCE_ORGANISM: &str = "organism";
pub const RESOURCE_ALLOY: &str = "alloy";
pub const RESOURCE_TUBE: &str = "tube";
pub const RESOURCE_FIXTURES: &str = "fixtures";
pub const RESOURCE_FRAME: &str = "frame";
pub const RESOURCE_HYDRAULICS: &str = "hydraulics";
pub const RESOURCE_MACHINE: &str = "machine";
pub const RESOURCE_CONDENSATE: &str = "condensate";
pub const RESOURCE_CONCENTRATE: &str = "concentrate";
pub const RESOURCE_EXTRACT: &str = "extract";
pub const RESOURCE_SPIRIT: &str = "spirit";
pub const RESOURCE_EMANATION: &str = "emanation";
pub const RESOURCE_ESSENCE: &str = "essence";
pub const REACTIONS: &str = r##"{"H":{"O":"OH","L":"LH","K":"KH","U":"UH","Z":"ZH","G":"GH"},"O":{"H":"OH","L":"LO","K":"KO","U":"UO","Z":"ZO","G":"GO"},"Z":{"K":"ZK","H":"ZH","O":"ZO"},"L":{"U":"UL","H":"LH","O":"LO"},"K":{"Z":"ZK","H":"KH","O":"KO"},"G":{"H":"GH","O":"GO"},"U":{"L":"UL","H":"UH","O":"UO"},"OH":{"UH":"UH2O","UO":"UHO2","ZH":"ZH2O","ZO":"ZHO2","KH":"KH2O","KO":"KHO2","LH":"LH2O","LO":"LHO2","GH":"GH2O","GO":"GHO2"},"X":{"UH2O":"XUH2O","UHO2":"XUHO2","LH2O":"XLH2O","LHO2":"XLHO2","KH2O":"XKH2O","KHO2":"XKHO2","ZH2O":"XZH2O","ZHO2":"XZHO2","GH2O":"XGH2O","GHO2":"XGHO2"},"ZK":{"UL":"G"},"UL":{"ZK":"G"},"LH":{"OH":"LH2O"},"ZH":{"OH":"ZH2O"},"GH":{"OH":"GH2O"},"KH":{"OH":"KH2O"},"UH":{"OH":"UH2O"},"LO":{"OH":"LHO2"},"ZO":{"OH":"ZHO2"},"KO":{"OH":"KHO2"},"UO":{"OH":"UHO2"},"GO":{"OH":"GHO2"},"LH2O":{"X":"XLH2O"},"KH2O":{"X":"XKH2O"},"ZH2O":{"X":"XZH2O"},"UH2O":{"X":"XUH2O"},"GH2O":{"X":"XGH2O"},"LHO2":{"X":"XLHO2"},"UHO2":{"X":"XUHO2"},"KHO2":{"X":"XKHO2"},"ZHO2":{"X":"XZHO2"},"GHO2":{"X":"XGHO2"}}"##;
pub const BOOSTS: &str = r##"{"work":{"UO":{"harvest":3},"UHO2":{"harvest":5},"XUHO2":{"harvest":7},"LH":{"build":1.5,"repair":1.5},"LH2O":{"build":1.8,"repair":1.8},"XLH2O":{"build":2,"repair":2},"ZH":{"dismantle":2},"ZH2O":{"dismantle":3},"XZH2O":{"dismantle":4},"GH":{"upgradeController":1.5},"GH2O":{"upgradeController":1.8},"XGH2O":{"upgradeController":2}},"attack":{"UH":{"attack":2},"UH2O":{"attack":3},"XUH2O":{"attack":4}},"ranged_attack":{"KO":{"rangedAttack":2,"rangedMassAttack":2},"KHO2":{"rangedAttack":3,"rangedMassAttack":3},"XKHO2":{"rangedAttack":4,"rangedMassAttack":4}},"heal":{"LO":{"heal":2,"rangedHeal":2},"LHO2":{"heal":3,"rangedHeal":3},"XLHO2":{"heal":4,"rangedHeal":4}},"carry":{"KH":{"capacity":2},"KH2O":{"capacity":3},"XKH2O":{"capacity":4}},"move":{"ZO":{"fatigue":2},"ZHO2":{"fatigue":3},"XZHO2":{"fatigue":4}},"tough":{"GO":{"damage":0.7},"GHO2":{"damage":0.5},"XGHO2":{"damage":0.3}}}"##;
pub const REACTION_TIME: &[(&str, i64)] = &[
    ("OH", 20i64),
    ("ZK", 5i64),
    ("UL", 5i64),
    ("G", 5i64),
    ("UH", 10i64),
    ("UH2O", 5i64),
    ("XUH2O", 60i64),
    ("UO", 10i64),
    ("UHO2", 5i64),
    ("XUHO2", 60i64),
    ("KH", 10i64),
    ("KH2O", 5i64),
    ("XKH2O", 60i64),
    ("KO", 10i64),
    ("KHO2", 5i64),
    ("XKHO2", 60i64),
    ("LH", 15i64),
    ("LH2O", 10i64),
    ("XLH2O", 65i64),
    ("LO", 10i64),
    ("LHO2", 5i64),
    ("XLHO2", 60i64),
    ("ZH", 20i64),
    ("ZH2O", 40i64),
    ("XZH2O", 160i64),
    ("ZO", 10i64),
    ("ZHO2", 5i64),
    ("XZHO2", 60i64),
    ("GH", 10i64),
    ("GH2O", 15i64),
    ("XGH2O", 80i64),
    ("GO", 10i64),
    ("GHO2", 30i64),
    ("XGHO2", 150i64),
];
pub const PORTAL_UNSTABLE: i64 = 864000000i64;
pub const PORTAL_MIN_TIMEOUT: i64 = 1036800000i64;
pub const PORTAL_MAX_TIMEOUT: i64 = 1900800000i64;
pub const POWER_BANK_RESPAWN_TIME: i64 = 50000i64;
pub const INVADERS_ENERGY_GOAL: i64 = 100000i64;
pub const SYSTEM_USERNAME: &str = "Screeps";
pub const SIGN_NOVICE_AREA: &str = "A new Novice or Respawn Area is being planned somewhere in this sector. Please make sure all important rooms are reserved.";
pub const SIGN_RESPAWN_AREA: &str = "A new Novice or Respawn Area is being planned somewhere in this sector. Please make sure all important rooms are reserved.";
pub const SIGN_PLANNED_AREA: &str = "A new Novice or Respawn Area is being planned somewhere in this sector. Please make sure all important rooms are reserved.";
pub const EVENT_ATTACK: i64 = 1i64;
pub const EVENT_OBJECT_DESTROYED: i64 = 2i64;
pub const EVENT_ATTACK_CONTROLLER: i64 = 3i64;
pub const EVENT_BUILD: i64 = 4i64;
pub const EVENT_HARVEST: i64 = 5i64;
pub const EVENT_HEAL: i64 = 6i64;
pub const EVENT_REPAIR: i64 = 7i64;
pub const EVENT_RESERVE_CONTROLLER: i64 = 8i64;
pub const EVENT_UPGRADE_CONTROLLER: i64 = 9i64;
pub const EVENT_EXIT: i64 = 10i64;
pub const EVENT_POWER: i64 = 11i64;
pub const EVENT_TRANSFER: i64 = 12i64;
pub const EVENT_ATTACK_TYPE_MELEE: i64 = 1i64;
pub const EVENT_ATTACK_TYPE_RANGED: i64 = 2i64;
pub const EVENT_ATTACK_TYPE_RANGED_MASS: i64 = 3i64;
pub const EVENT_ATTACK_TYPE_DISMANTLE: i64 = 4i64;
pub const EVENT_ATTACK_TYPE_HIT_BACK: i64 = 5i64;
pub const EVENT_ATTACK_TYPE_NUKE: i64 = 6i64;
pub const EVENT_HEAL_TYPE_MELEE: i64 = 1i64;
pub const EVENT_HEAL_TYPE_RANGED: i64 = 2i64;
pub const POWER_LEVEL_MULTIPLY: i64 = 1000i64;
pub const POWER_LEVEL_POW: i64 = 2i64;
pub const POWER_CREEP_SPAWN_COOLDOWN: i64 = 28800000i64;
pub const POWER_CREEP_DELETE_COOLDOWN: i64 = 86400000i64;
pub const POWER_CREEP_MAX_LEVEL: i64 = 25i64;
pub const POWER_CREEP_LIFE_TIME: i64 = 5000i64;
pub const POWER_CLASS: &[(&str, &str)] = &[("OPERATOR", "operator")];
pub const PWR_GENERATE_OPS: i64 = 1i64;
pub const PWR_OPERATE_SPAWN: i64 = 2i64;
pub const PWR_OPERATE_TOWER: i64 = 3i64;
pub const PWR_OPERATE_STORAGE: i64 = 4i64;
pub const PWR_OPERATE_LAB: i64 = 5i64;
pub const PWR_OPERATE_EXTENSION: i64 = 6i64;
pub const PWR_OPERATE_OBSERVER: i64 = 7i64;
pub const PWR_OPERATE_TERMINAL: i64 = 8i64;
pub const PWR_DISRUPT_SPAWN: i64 = 9i64;
pub const PWR_DISRUPT_TOWER: i64 = 10i64;
pub const PWR_DISRUPT_SOURCE: i64 = 11i64;
pub const PWR_SHIELD: i64 = 12i64;
pub const PWR_REGEN_SOURCE: i64 = 13i64;
pub const PWR_REGEN_MINERAL: i64 = 14i64;
pub const PWR_DISRUPT_TERMINAL: i64 = 15i64;
pub const PWR_OPERATE_POWER: i64 = 16i64;
pub const PWR_FORTIFY: i64 = 17i64;
pub const PWR_OPERATE_CONTROLLER: i64 = 18i64;
pub const PWR_OPERATE_FACTORY: i64 = 19i64;
pub const EFFECT_INVULNERABILITY: i64 = 1001i64;
pub const EFFECT_COLLAPSE_TIMER: i64 = 1002i64;
pub const INVADER_CORE_HITS: i64 = 100000i64;
pub const INVADER_CORE_CREEP_SPAWN_TIME: &[(&str, i64)] = &[
    ("0", 0i64),
    ("1", 0i64),
    ("2", 6i64),
    ("3", 3i64),
    ("4", 2i64),
    ("5", 1i64),
];
pub const INVADER_CORE_EXPAND_TIME: &[(&str, i64)] = &[
    ("1", 4000i64),
    ("2", 3500i64),
    ("3", 3000i64),
    ("4", 2500i64),
    ("5", 2000i64),
];
pub const INVADER_CORE_CONTROLLER_POWER: i64 = 2i64;
pub const INVADER_CORE_CONTROLLER_DOWNGRADE: i64 = 5000i64;
pub const STRONGHOLD_RAMPART_HITS: &[(&str, i64)] = &[
    ("0", 0i64),
    ("1", 100000i64),
    ("2", 200000i64),
    ("3", 500000i64),
    ("4", 1000000i64),
    ("5", 2000000i64),
];
pub const STRONGHOLD_DECAY_TICKS: i64 = 75000i64;
pub const POWER_INFO: &str = r##"{"1":{"className":"operator","level":[0,2,7,14,22],"cooldown":50,"effect":[1,2,4,6,8]},"2":{"className":"operator","level":[0,2,7,14,22],"cooldown":300,"duration":1000,"range":3,"ops":100,"effect":[0.9,0.7,0.5,0.35,0.2]},"3":{"className":"operator","level":[0,2,7,14,22],"cooldown":10,"duration":100,"range":3,"ops":10,"effect":[1.1,1.2,1.3,1.4,1.5]},"4":{"className":"operator","level":[0,2,7,14,22],"cooldown":800,"duration":1000,"range":3,"ops":100,"effect":[500000,1000000,2000000,4000000,7000000]},"5":{"className":"operator","level":[0,2,7,14,22],"cooldown":50,"duration":1000,"range":3,"ops":10,"effect":[2,4,6,8,10]},"6":{"className":"operator","level":[0,2,7,14,22],"cooldown":50,"range":3,"ops":2,"effect":[0.2,0.4,0.6,0.8,1]},"7":{"className":"operator","level":[0,2,7,14,22],"cooldown":400,"duration":[200,400,600,800,1000],"range":3,"ops":10},"8":{"className":"operator","level":[0,2,7,14,22],"cooldown":500,"duration":1000,"range":3,"ops":100,"effect":[0.9,0.8,0.7,0.6,0.5]},"9":{"className":"operator","level":[0,2,7,14,22],"cooldown":5,"range":20,"ops":10,"duration":[1,2,3,4,5]},"10":{"className":"operator","level":[0,2,7,14,22],"cooldown":0,"duration":5,"range":50,"ops":10,"effect":[0.9,0.8,0.7,0.6,0.5]},"11":{"className":"operator","level":[0,2,7,14,22],"cooldown":100,"range":3,"ops":100,"duration":[100,200,300,400,500]},"12":{"className":"operator","level":[0,2,7,14,22],"effect":[5000,10000,15000,20000,25000],"duration":50,"cooldown":20,"energy":100},"13":{"className":"operator","level":[10,11,12,14,22],"cooldown":100,"duration":300,"range":3,"effect":[50,100,150,200,250],"period":15},"14":{"className":"operator","level":[10,11,12,14,22],"cooldown":100,"duration":100,"range":3,"effect":[2,4,6,8,10],"period":10},"15":{"className":"operator","level":[20,21,22,23,24],"cooldown":8,"duration":10,"range":50,"ops":[50,40,30,20,10]},"16":{"className":"operator","level":[10,11,12,14,22],"cooldown":800,"range":3,"duration":1000,"ops":200,"effect":[1,2,3,4,5]},"17":{"className":"operator","level":[0,2,7,14,22],"cooldown":5,"range":3,"ops":5,"duration":[1,2,3,4,5]},"18":{"className":"operator","level":[20,21,22,23,24],"cooldown":800,"range":3,"duration":1000,"ops":200,"effect":[10,20,30,40,50]},"19":{"className":"operator","level":[0,2,7,14,22],"cooldown":800,"range":3,"duration":1000,"ops":100}}"##;
pub const BODYPARTS_ALL: &[&str] = &[
    "move",
    "work",
    "carry",
    "attack",
    "ranged_attack",
    "tough",
    "heal",
    "claim",
];
pub const RESOURCES_ALL: &[&str] = &[
    "energy",
    "power",
    "H",
    "O",
    "U",
    "K",
    "L",
    "Z",
    "X",
    "G",
    "OH",
    "ZK",
    "UL",
    "UH",
    "UO",
    "KH",
    "KO",
    "LH",
    "LO",
    "ZH",
    "ZO",
    "GH",
    "GO",
    "UH2O",
    "UHO2",
    "KH2O",
    "KHO2",
    "LH2O",
    "LHO2",
    "ZH2O",
    "ZHO2",
    "GH2O",
    "GHO2",
    "XUH2O",
    "XUHO2",
    "XKH2O",
    "XKHO2",
    "XLH2O",
    "XLHO2",
    "XZH2O",
    "XZHO2",
    "XGH2O",
    "XGHO2",
    "ops",
    "silicon",
    "metal",
    "biomass",
    "mist",
    "utrium_bar",
    "lemergium_bar",
    "zynthium_bar",
    "keanium_bar",
    "ghodium_melt",
    "oxidant",
    "reductant",
    "purifier",
    "battery",
    "composite",
    "crystal",
    "liquid",
    "wire",
    "switch",
    "transistor",
    "microchip",
    "circuit",
    "device",
    "cell",
    "phlegm",
    "tissue",
    "muscle",
    "organoid",
    "organism",
    "alloy",
    "tube",
    "fixtures",
    "frame",
    "hydraulics",
    "machine",
    "condensate",
    "concentrate",
    "extract",
    "spirit",
    "emanation",
    "essence",
];
pub const COLORS_ALL: &[i64] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
pub const INTERSHARD_RESOURCES: &[&str] = &["token", "cpuUnlock", "pixel", "accessKey"];
pub const COMMODITIES: &str = r##"{"utrium_bar":{"amount":100,"cooldown":20,"components":{"U":500,"energy":200}},"U":{"amount":500,"cooldown":20,"components":{"utrium_bar":100,"energy":200}},"lemergium_bar":{"amount":100,"cooldown":20,"components":{"L":500,"energy":200}},"L":{"amount":500,"cooldown":20,"components":{"lemergium_bar":100,"energy":200}},"zynthium_bar":{"amount":100,"cooldown":20,"components":{"Z":500,"energy":200}},"Z":{"amount":500,"cooldown":20,"components":{"zynthium_bar":100,"energy":200}},"keanium_bar":{"amount":100,"cooldown":20,"components":{"K":500,"energy":200}},"K":{"amount":500,"cooldown":20,"components":{"keanium_bar":100,"energy":200}},"ghodium_melt":{"amount":100,"cooldown":20,"components":{"G":500,"energy":200}},"G":{"amount":500,"cooldown":20,"components":{"ghodium_melt":100,"energy":200}},"oxidant":{"amount":100,"cooldown":20,"components":{"O":500,"energy":200}},"O":{"amount":500,"cooldown":20,"components":{"oxidant":100,"energy":200}},"reductant":{"amount":100,"cooldown":20,"components":{"H":500,"energy":200}},"H":{"amount":500,"cooldown":20,"components":{"reductant":100,"energy":200}},"purifier":{"amount":100,"cooldown":20,"components":{"X":500,"energy":200}},"X":{"amount":500,"cooldown":20,"components":{"purifier":100,"energy":200}},"battery":{"amount":50,"cooldown":10,"components":{"energy":600}},"energy":{"amount":500,"cooldown":10,"components":{"battery":50}},"composite":{"level":1,"amount":20,"cooldown":50,"components":{"utrium_bar":20,"zynthium_bar":20,"energy":20}},"crystal":{"level":2,"amount":6,"cooldown":21,"components":{"lemergium_bar":6,"keanium_bar":6,"purifier":6,"energy":45}},"liquid":{"level":3,"amount":12,"cooldown":60,"components":{"oxidant":12,"reductant":12,"ghodium_melt":12,"energy":90}},"wire":{"amount":20,"cooldown":8,"components":{"utrium_bar":20,"silicon":100,"energy":40}},"switch":{"level":1,"amount":5,"cooldown":70,"components":{"wire":40,"oxidant":95,"utrium_bar":35,"energy":20}},"transistor":{"level":2,"amount":1,"cooldown":59,"components":{"switch":4,"wire":15,"reductant":85,"energy":8}},"microchip":{"level":3,"amount":1,"cooldown":250,"components":{"transistor":2,"composite":50,"wire":117,"purifier":25,"energy":16}},"circuit":{"level":4,"amount":1,"cooldown":800,"components":{"microchip":1,"transistor":5,"switch":4,"oxidant":115,"energy":32}},"device":{"level":5,"amount":1,"cooldown":600,"components":{"circuit":1,"microchip":3,"crystal":110,"ghodium_melt":150,"energy":64}},"cell":{"amount":20,"cooldown":8,"components":{"lemergium_bar":20,"biomass":100,"energy":40}},"phlegm":{"level":1,"amount":2,"cooldown":35,"components":{"cell":20,"oxidant":36,"lemergium_bar":16,"energy":8}},"tissue":{"level":2,"amount":2,"cooldown":164,"components":{"phlegm":10,"cell":10,"reductant":110,"energy":16}},"muscle":{"level":3,"amount":1,"cooldown":250,"components":{"tissue":3,"phlegm":3,"zynthium_bar":50,"reductant":50,"energy":16}},"organoid":{"level":4,"amount":1,"cooldown":800,"components":{"muscle":1,"tissue":5,"purifier":208,"oxidant":256,"energy":32}},"organism":{"level":5,"amount":1,"cooldown":600,"components":{"organoid":1,"liquid":150,"tissue":6,"cell":310,"energy":64}},"alloy":{"amount":20,"cooldown":8,"components":{"zynthium_bar":20,"metal":100,"energy":40}},"tube":{"level":1,"amount":2,"cooldown":45,"components":{"alloy":40,"zynthium_bar":16,"energy":8}},"fixtures":{"level":2,"amount":1,"cooldown":115,"components":{"composite":20,"alloy":41,"oxidant":161,"energy":8}},"frame":{"level":3,"amount":1,"cooldown":125,"components":{"fixtures":2,"tube":4,"reductant":330,"zynthium_bar":31,"energy":16}},"hydraulics":{"level":4,"amount":1,"cooldown":800,"components":{"liquid":150,"fixtures":3,"tube":15,"purifier":208,"energy":32}},"machine":{"level":5,"amount":1,"cooldown":600,"components":{"hydraulics":1,"frame":2,"fixtures":3,"tube":12,"energy":64}},"condensate":{"amount":20,"cooldown":8,"components":{"keanium_bar":20,"mist":100,"energy":40}},"concentrate":{"level":1,"amount":3,"cooldown":41,"components":{"condensate":30,"keanium_bar":15,"reductant":54,"energy":12}},"extract":{"level":2,"amount":2,"cooldown":128,"components":{"concentrate":10,"condensate":30,"oxidant":60,"energy":16}},"spirit":{"level":3,"amount":1,"cooldown":200,"components":{"extract":2,"concentrate":6,"reductant":90,"purifier":20,"energy":16}},"emanation":{"level":4,"amount":1,"cooldown":800,"components":{"spirit":2,"extract":2,"concentrate":3,"keanium_bar":112,"energy":32}},"essence":{"level":5,"amount":1,"cooldown":600,"components":{"emanation":1,"spirit":3,"crystal":110,"ghodium_melt":150,"energy":64}}}"##;
