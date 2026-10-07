/* Mod 玩家面安装运行时。仅在宿主声明分片含 Mod 玩家面时装配
 * （SandboxConfig.mod_api）——vanilla 分片的全局面保持不变。
 * 执行序：全部 prelude 段与扩展意图段之后、宿主句柄捕获之前。
 * 宿主先注入清单（globalThis.__kkModApiManifest，本段入口即取即删），
 * 再逐包以 createInstallApi(namespace) 产出安装 API、经 ESM 装载器调用
 * 包入口的 default(api)，全部模块完成后 finalize 校验必需绑定并登记
 * gw.modApi 供对象装配与 FIND/LOOK 查询使用；建筑类型同样经
 * api.buildings.bind 绑定玩家类，登记的寻路语义与成本键供工地预检、
 * 代价矩阵与 transfer 白名单消费。
 * api 对象的权限止于闭包引用（gw 与安装期捕获的内建类）；__kkp 删除后
 * 依然可用，玩家后续的原型改写不影响安装结果。
 */
;(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const manifest = globalThis.__kkModApiManifest;
  delete globalThis.__kkModApiManifest;
  const __Object = Object;
  const __defineProperty = __Object.defineProperty;
  const __getOwnPropertyNames = __Object.getOwnPropertyNames;
  const __keys = __Object.keys;
  const __Set = Set;

  // 类型键 → { find, look, cls }；findByConst 把 FIND 常量映回类型键。
  const types = __Object.create(null);
  const findByConst = __Object.create(null);
  for (let i = 0; i < manifest.types.length; i++) {
    const t = manifest.types[i];
    types[t.key] = { find: t.find, look: t.look, cls: null };
    findByConst[t.find] = t.key;
  }
  // 建筑类型键 → { pathing, cost, fields, cls }：完工结构记录的
  // structureType 命中本表时使用绑定玩家类；寻路语义与成本键供预检/
  // 代价矩阵消费；fields 是自定义字段存储键（含命名空间冒号），
  // 装配器按记录直供同名属性（首片可见性仅 public，全字段直供）。
  const buildings = __Object.create(null);
  for (let i = 0; i < manifest.buildings.length; i++) {
    const b = manifest.buildings[i];
    buildings[b.key] = { pathing: b.pathing, cost: b.cost, fields: b.fields || [], cls: null };
  }
  // emit 目录按安装包的命名空间裁剪（纵深防御：A 包的模块不经 B 包的
  // 键发出意图；结算侧权威校验仍同源全目录）。
  const allIntents = manifest.intents;
  // 意图目录非空时宿主必须已装配原语（两处同源于冻结计划）；缺失即
  // 装配接线缺陷，安装期失败好过玩家运行期静默丢意图。
  const emitPrimitive = globalThis.__kkEmitExtensionIntent;
  if (allIntents.length > 0 && typeof emitPrimitive !== 'function') {
    throw new Error('Mod 意图目录非空但 __kkEmitExtensionIntent 原语缺席（宿主装配缺陷）');
  }

  // Mod 玩家类可继承的内建类集合：安装期捕获（全部 prelude 段已完成），
  // 玩家随后替换全局绑定不影响已捕获引用。
  const BUILTIN_CLASS_NAMES = [
    'ConstructionSite', 'Creep', 'Deposit', 'Flag', 'Mineral', 'Nuke',
    'OwnedStructure', 'PowerCreep', 'Resource', 'Ruin', 'Room', 'RoomObject',
    'RoomPosition', 'Source', 'Store', 'Structure', 'StructureContainer',
    'StructureController', 'StructureExtension', 'StructureExtractor',
    'StructureFactory', 'StructureInvaderCore', 'StructureKeeperLair',
    'StructureLab', 'StructureLink', 'StructureNuker', 'StructureObserver',
    'StructurePortal', 'StructurePowerBank', 'StructurePowerSpawn',
    'StructureRampart', 'StructureRoad', 'StructureSpawn', 'StructureStorage',
    'StructureTerminal', 'StructureTower', 'StructureWall', 'Tombstone',
  ];
  const builtins = __Object.create(null);
  for (let i = 0; i < BUILTIN_CLASS_NAMES.length; i++) {
    const name = BUILTIN_CLASS_NAMES[i];
    const cls = globalThis[name];
    if (typeof cls !== 'function') {
      throw new Error('玩家 API 安装期内建类 ' + name + ' 缺席（段装配序缺陷）');
    }
    builtins[name] = cls;
  }
  __Object.freeze(builtins);

  const definedGlobals = [];
  const C_OK = globalThis.OK;
  const C_ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;

  function createInstallApi(namespace) {
    const prefix = namespace + ':';
    const intents = new __Set(
      allIntents.filter(function (key) { return typeof key === 'string' && key.indexOf(prefix) === 0; })
    );
    function bind(typeKey, cls) {
      if (typeof typeKey !== 'string' || types[typeKey] === undefined) {
        throw new Error('类型 ' + JSON.stringify(typeKey) + ' 不在冻结计划中（不能绑定玩家类）');
      }
      // 与 emit 目录同律：只能绑定本包命名空间的类型，防止先装载的包
      // 抢绑他人类型的玩家类（宿主内容均受信，此处是纵深防御）。
      if (typeKey.indexOf(prefix) !== 0) {
        throw new Error('类型 ' + JSON.stringify(typeKey) + ' 不属于本包命名空间 ' + namespace);
      }
      if (typeof cls !== 'function') {
        throw new Error('玩家类绑定需要一个构造器（' + namespace + ' → ' + typeKey + '）');
      }
      if (types[typeKey].cls !== null) {
        throw new Error('类型 ' + typeKey + ' 已绑定玩家类（不得重复绑定）');
      }
      types[typeKey].cls = cls;
    }

    // 建筑玩家类绑定：与对象绑定同律（清单内类型、构造器、不得重复）。
    function bindBuilding(typeKey, cls) {
      if (typeof typeKey !== 'string' || buildings[typeKey] === undefined) {
        throw new Error('建筑 ' + JSON.stringify(typeKey) + ' 不在冻结计划中（不能绑定玩家类）');
      }
      if (typeKey.indexOf(prefix) !== 0) {
        throw new Error('建筑 ' + JSON.stringify(typeKey) + ' 不属于本包命名空间 ' + namespace);
      }
      if (typeof cls !== 'function') {
        throw new Error('建筑玩家类绑定需要一个构造器（' + namespace + ' → ' + typeKey + '）');
      }
      if (buildings[typeKey].cls !== null) {
        throw new Error('建筑 ' + typeKey + ' 已绑定玩家类（不得重复绑定）');
      }
      buildings[typeKey].cls = cls;
    }

    function defineGlobal(name, value) {
      if (typeof name !== 'string' || !/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name)) {
        throw new Error('全局名 ' + JSON.stringify(name) + ' 不是合法标识符');
      }
      if (__getOwnPropertyNames(globalThis).indexOf(name) !== -1) {
        throw new Error('全局 ' + name + ' 已存在（不得覆盖内建或其他 Mod 的安装结果）');
      }
      __defineProperty(globalThis, name, {
        enumerable: true, writable: false, configurable: false, value,
      });
      definedGlobals.push(name);
    }

    // 字段从当轮已授权投影读取（宿主已按可见性裁切）；不存在或不可见
    // 的字段读到 undefined，与缺键形态一致。
    const view = {
      field(obj, name) {
        if (obj === null || typeof obj !== 'object' || typeof obj.id !== 'string') {
          throw new Error('view.field 需要游戏对象（含 id）作为第一参数');
        }
        if (typeof name !== 'string') {
          throw new Error('view.field 需要字符串字段名');
        }
        return gw.rawData(obj.id)[name];
      },
    };

    // 预检只拦明显形态（键不在目录、目标非游戏对象）；参数 schema、
    // 接收者与归属由宿主结算侧独立校验，伪造最多得到一次不执行。
    const intentsApi = {
      emit(intentKey, target, args) {
        if (typeof intentKey !== 'string' || !intents.has(intentKey)) {
          return C_ERR_INVALID_ARGS;
        }
        if (target === null || typeof target !== 'object' || typeof target.id !== 'string') {
          return C_ERR_INVALID_ARGS;
        }
        const ok = emitPrimitive(intentKey, target.id, args === undefined ? null : args);
        return ok ? C_OK : C_ERR_INVALID_ARGS;
      },
    };

    const queries = {
      install(typeKey) {
        const t = types[typeKey];
        if (t === undefined) {
          throw new Error('类型 ' + JSON.stringify(typeKey) + ' 不在冻结计划中（不能安装查询常量）');
        }
        return { find: t.find, look: t.look };
      },
    };

    return __Object.freeze({
      builtins,
      objects: __Object.freeze({ bind }),
      buildings: __Object.freeze({ bind: bindBuilding }),
      globals: __Object.freeze({ define: defineGlobal }),
      view,
      intents: intentsApi,
      queries,
    });
  }

  // 全部玩家模块执行完毕后调用：玩家面类型必须有类（ServerOnly 类型
  // 不进清单，天然免绑）；缺绑定即分片创建失败，不静默产出不可用类型。
  function finalize() {
    const missing = [];
    const keys = __keys(types);
    for (let i = 0; i < keys.length; i++) {
      if (types[keys[i]].cls === null) missing.push(keys[i]);
    }
    if (missing.length > 0) {
      throw new Error('玩家模块未绑定玩家面类型：' + missing.join(', ')
        + '（声明包须在 player.js 内 api.objects.bind）');
    }
    const missingBuildings = [];
    const buildingKeys = __keys(buildings);
    for (let i = 0; i < buildingKeys.length; i++) {
      if (buildings[buildingKeys[i]].cls === null) missingBuildings.push(buildingKeys[i]);
    }
    if (missingBuildings.length > 0) {
      throw new Error('玩家模块未绑定建筑玩家类：' + missingBuildings.join(', ')
        + '（声明包须在 player.js 内 api.buildings.bind）');
    }
    // modFindBase 是 Rust 清单权威下界（Rust 侧唯一来源，沙箱不重复
    // 硬编码）；无类型时为 Infinity——mod 查询分支恒不命中。
    gw.modApi = {
      types, findByConst,
      modFindBase: manifest.findBase === undefined ? Infinity : manifest.findBase,
      buildings,
    };
    return { globals: definedGlobals.slice(), types: keys };
  }

  globalThis.__kkp.modApiRuntime = { createInstallApi, finalize };
})();
