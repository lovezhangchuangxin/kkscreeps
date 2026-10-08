/* 单房间查询上下文拥有 look 类型/空间索引与出口缓存；每 tick 重新构造。
 * 原始对象列表仅交给创建预检与寻路缓存，索引不暴露为玩家属性。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const { countStub, lodash: _ } = globalThis.__kkp;
  const __mapGet = Map.prototype.get;
  const __reflectApply = Reflect.apply;
  // 装配索引使用捕获的 push，避免玩家替换数组原型破坏下一 tick。
  const __arrayPush = Array.prototype.push;
  const ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  const terrainWordAt = gw.terrainWordAt;
  gw.makeRoomQueries = function (name) {
    // 无 controller 房间的官方投影是 undefined（非 null）
    let controller;
    const worldObjs = gw.world.objects || [];
    // 一次扫描本房对象建立类型索引与 x*50+y 的空间索引，同时寻找 controller。
    // energy/resource 共用索引，lookAt 同格掉落资源会输出两项。
    // 来源：上游 engine/src/game/rooms.js 的 lookTypeRegisters/lookTypeSpatialRegisters。
    const lookReg = {
      creep: {},
      source: {},
      mineral: {},
      deposit: {},
      structure: {},
      flag: {},
      constructionSite: {},
      tombstone: {},
      ruin: {},
      nuke: {},
      powerCreep: {},
    };
    lookReg.energy = {};
    lookReg.resource = lookReg.energy;
    const lookSpatial = {
      creep: [],
      source: [],
      mineral: [],
      deposit: [],
      structure: [],
      flag: [],
      constructionSite: [],
      tombstone: [],
      ruin: [],
      nuke: [],
      powerCreep: [],
    };
    // 保留本房原始对象供工地名额与占位检查；storage/terminal 另记录其玩家视图引用。
    const roomObjs = [];
    let storageObj, terminalObj;
    lookSpatial.energy = [];
    lookSpatial.resource = lookSpatial.energy;
    // Mod look 桶按玩家面目录预初始化（与内建 13 桶同等待遇）：空房
    // lookForAt 返回 [] 而不是 ERR_INVALID_ARGS；类型键来自注册目录
    //（恒含冒号），不与内建键或 Object.prototype 原型键冲突。
    const modLookTypes = [];
    if (gw.modApi !== undefined) {
      for (const typeKey in gw.modApi.types) {
        lookReg[typeKey] = {};
        lookSpatial[typeKey] = [];
        modLookTypes.push(typeKey);
      }
      modLookTypes.sort();
    }
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      if (o.room !== name) continue;
      roomObjs.push(o);
      if (o.type === 'structure' && o.structureType === 'controller') {
        controller = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      }
      if (o.type === 'structure' && o.structureType === 'storage') {
        storageObj = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      }
      if (o.type === 'structure' && o.structureType === 'terminal') {
        terminalObj = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      }
      // look 类型归类（官方装配分支同构：source/mineral 独立臂、spawn/controller
      // 属 structure、dropped resource 落 energy 臂）
      let t = null;
      if (o.type === 'creep') t = 'creep';
      else if (o.type === 'resource') t = 'energy';
      else if (o.type === 'structure') {
        if (o.structureType === 'source') t = 'source';
        else if (o.structureType === 'mineral') t = 'mineral';
        else t = 'structure';
      } else if (o.type === 'spawn') t = 'structure';
      else if (o.type === 'flag') t = 'flag';
      else if (o.type === 'constructionSite') t = 'constructionSite';
      else if (o.type === 'tombstone') t = 'tombstone';
      else if (o.type === 'ruin') t = 'ruin';
      else if (o.type === 'nuke') t = 'nuke';
      else if (o.type === 'powerCreep') t = 'powerCreep';
      else if (o.type === 'deposit') t = 'deposit';
      // Mod 对象按类型键进 look 桶（键带冒号，不与内建 look 键冲突；
      // 桶已在下方按玩家面目录预初始化）。未安装玩家面的分片不会有
      // mod 记录，此分支为死代码。
      else if (o.type === 'mod') t = o.modType;
      if (t === null) continue;
      const w = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      if (!w) continue;
      lookReg[t][o.id] = w;
      const index = o.x * 50 + o.y;
      if (lookSpatial[t][index] === undefined) {
        lookSpatial[t][index] = [w];
      } else {
        __reflectApply(__arrayPush, lookSpatial[t][index], [w]);
      }
    }
    return {
      controller,
      storageObj,
      terminalObj,
      roomObjs,
      install(room) {
        // 方向出口缓存未过滤结果；FIND_EXIT 首次合并的缓存已经过当次 filter。
        // 再次调用的 filter 作用于该缓存，不能恢复首次已过滤掉的出口。
        // 返回新数组但复用元素引用；对象类查询每次扫描本房对象。
        // 来源：上游 engine/src/game/rooms.js 的 Room.find。
        const findCache = {};
        room.find = function (type, opts) {
          countStub('room.find:' + type);
          // 官方 rooms.js:592-601：FIND_EXIT = 四向臂 concat（官方序
          // TOP/BOTTOM/RIGHT/LEFT），子臂各自应用 opts（含 filter）；缓存后
          // 提前 return _.clone（不走通用尾滤——陈旧 filter 语义在此形成）
          if (type === 10 /* FIND_EXIT */) {
            if (findCache[10] !== undefined) {
              // 官方命中汇入通用尾（rooms.js:587-588→:648-655）：当次 filter
              // 作用于烘焙缓存（交集），无 filter 走 _.clone 新数组
              if (opts && opts.filter) return _.filter(findCache[10], opts.filter);
              return _.clone(findCache[10]);
            }
            const r = this.find(1, opts)
              .concat(this.find(5, opts))
              .concat(this.find(3, opts))
              .concat(this.find(7, opts));
            findCache[10] = r;
            return _.clone(r);
          }
          // 官方 rooms.js:602-641：四向臂由地形 wall 位计算——沿边 50 格，
          // 非 wall 者经 getPositionAt 产出（top: (key,0)、bottom: (key,49)、
          // left: (0,key)、right: (49,key)）。terrain flat 数组行主序 y*50+x
          // （非转置）；未滤结果入缓存，命中走通用尾滤
          if (type === 1 || type === 3 || type === 5 || type === 7) {
            if (findCache[type] !== undefined) {
              if (opts && opts.filter) return _.filter(findCache[type], opts.filter);
              return _.clone(findCache[type]);
            }
            const terrain = gw.terrainByRoom[name];
            const exits = [];
            for (let i = 0; i < 50; i++) {
              let x = 0,
                y = 0;
              if (type === 7 || type === 3) {
                y = i;
              } else {
                x = i;
              }
              if (type === 3) {
                x = 49;
              }
              if (type === 5) {
                y = 49;
              }
              exits.push(!(terrain[y * 50 + x] & TERRAIN_MASK_WALL));
            }
            const out = [];
            for (let key = 0; key < exits.length; key++) {
              if (exits[key]) {
                if (type === 1) out.push(this.getPositionAt(key, 0));
                if (type === 5) out.push(this.getPositionAt(key, 49));
                if (type === 7) out.push(this.getPositionAt(0, key));
                if (type === 3) out.push(this.getPositionAt(49, key));
              }
            }
            findCache[type] = out;
            if (opts && opts.filter) return _.filter(out, opts.filter);
            return _.clone(out);
          }
          const out = [];
          // Mod 查询常量：FIND 下界与常量→类型映射都来自玩家面目录（Rust
          // 清单权威）；下界判定与映射在循环外完成。
          const modFindType =
            gw.modApi === undefined || type < gw.modApi.modFindBase
              ? undefined
              : gw.modApi.findByConst[type];
          // 没有查看者时 MY_*/HOSTILE_* 无法分类，返回空；FIND_CREEPS 在此模式保留孵化对象。
          const neutral = gw.viewerName === null;
          for (let i = 0; i < worldObjs.length; i++) {
            const o = worldObjs[i];
            if (o.room !== name) continue;
            const w = __reflectApply(__mapGet, gw.objectsById, [o.id]);
            if (!w) continue;
            const isStruct = o.type === 'structure' || o.type === 'spawn';
            // 官方 findCacheFn 表（game.js:17-29）viewer 相关 10 臂：FIND_CREEPS
            // 族滤孵化雏形（!i.spawning——中立面保留）；MY_*=真值 my、HOSTILE_*
            // =严格 false 混合（可达状态等价——creep/spawn owner 恒字符串、site
            // null 投影缺键）；109 官方另有 `&& i.owner`（owner getter 对 null/
            // 缺键归 undefined，structures.js:136-138）→ 敌对结构臂只认字符串
            // owner（与 Rust hostile_of 同口径；cleared controller 的 owner:null
            // 双层双臂均不进）
            const creepOk = o.type === 'creep' && (neutral || !o.spawning);
            if (type === 101 /* FIND_CREEPS */ && creepOk) out.push(w);
            else if (
              type === 102 /* FIND_MY_CREEPS */ &&
              !neutral &&
              creepOk &&
              o.owner === gw.viewerName
            )
              out.push(w);
            else if (
              type === 103 /* FIND_HOSTILE_CREEPS */ &&
              !neutral &&
              creepOk &&
              typeof o.owner === 'string' &&
              o.owner !== gw.viewerName
            )
              out.push(w);
            else if (
              type === 104 /* FIND_SOURCES_ACTIVE */ &&
              o.type === 'structure' &&
              o.structureType === 'source' &&
              (o.energy ?? 3000) > 0
            )
              out.push(w);
            else if (
              type === 105 /* FIND_SOURCES */ &&
              o.type === 'structure' &&
              o.structureType === 'source'
            )
              out.push(w);
            else if (type === 106 /* FIND_DROPPED_RESOURCES */ && o.type === 'resource')
              out.push(w);
            // 官方 FIND_STRUCTURES/FIND_MY_STRUCTURES 不含 source/mineral：
            // game.js:229-251 structureTypes 表无此三类（独立装配分支 :329-345），
            // 与 Rust 侧 matches_find 两层一致
            else if (
              type === 107 /* FIND_STRUCTURES */ &&
              isStruct &&
              o.structureType !== 'source' &&
              o.structureType !== 'mineral'
            )
              out.push(w);
            else if (
              type === 108 /* FIND_MY_STRUCTURES */ &&
              !neutral &&
              isStruct &&
              o.structureType !== 'source' &&
              o.structureType !== 'mineral' &&
              o.owner === gw.viewerName
            )
              out.push(w);
            else if (
              type === 109 /* FIND_HOSTILE_STRUCTURES */ &&
              !neutral &&
              isStruct &&
              o.structureType !== 'source' &&
              o.structureType !== 'mineral' &&
              typeof o.owner === 'string' &&
              o.owner !== gw.viewerName
            )
              out.push(w);
            else if (type === 110 /* FIND_FLAGS */ && o.type === 'flag') out.push(w);
            else if (type === 111 /* FIND_CONSTRUCTION_SITES */ && o.type === 'constructionSite')
              out.push(w);
            else if (
              type === 112 /* FIND_MY_SPAWNS */ &&
              !neutral &&
              o.type === 'spawn' &&
              o.owner === gw.viewerName
            )
              out.push(w);
            else if (
              type === 113 /* FIND_HOSTILE_SPAWNS */ &&
              !neutral &&
              o.type === 'spawn' &&
              typeof o.owner === 'string' &&
              o.owner !== gw.viewerName
            )
              out.push(w);
            else if (
              type === 114 /* FIND_MY_CONSTRUCTION_SITES */ &&
              !neutral &&
              o.type === 'constructionSite' &&
              o.owner === gw.viewerName
            )
              out.push(w);
            else if (
              type === 115 /* FIND_HOSTILE_CONSTRUCTION_SITES */ &&
              !neutral &&
              o.type === 'constructionSite' &&
              typeof o.owner === 'string' &&
              o.owner !== gw.viewerName
            )
              out.push(w);
            else if (
              type === 116 /* FIND_MINERALS */ &&
              o.type === 'structure' &&
              o.structureType === 'mineral'
            )
              out.push(w);
            // 这些查询分支从当前世界对象中收集墓碑、powerCreep、deposit 与废墟；无对象时返回空。
            else if (type === 117 /* FIND_NUKES */ && o.type === 'nuke') out.push(w);
            else if (type === 118 /* FIND_TOMBSTONES */ && o.type === 'tombstone') out.push(w);
            else if (type === 119 /* FIND_POWER_CREEPS */ && o.type === 'powerCreep') out.push(w);
            else if (type === 122 /* FIND_DEPOSITS */ && o.type === 'deposit') out.push(w);
            else if (type === 123 /* FIND_RUINS */ && o.type === 'ruin') out.push(w);
            // Mod 查询常量（注册期分配，下界见 modFindBase）：命中对应类型。
            else if (modFindType !== undefined && o.type === 'mod' && o.modType === modFindType) {
              out.push(w);
            }
          }
          // 官方 rooms.js:648-654：opts.filter（函数或 lodash 对象谓词）经
          // _.filter 应用——bot 生态最常用的 find 形态
          if (opts && opts.filter) return _.filter(out, opts.filter);
          return out;
        };
        // 官方 rooms.js:659-707 _lookSpatialRegister 逐字：terrain 分支读静态地形
        //（越界索引落 flat 数组外 → undefined 掩码得 'plain'，官方怪癖保留；无地形
        // 房间自然 TypeError 同构）；非 terrain 越界抛官方文案；outArray 形态产出
        // {type,[type]:obj}（lookAtArea asArray 加 x/y），否则 [单词]/_.clone(数组)/[]
        function _lookSpatialRegister(typeName, x, y, outArray, withCoords) {
          let item;
          if (typeName == 'terrain') {
            const word = terrainWordAt(name, x, y);
            if (outArray) {
              item = { type: 'terrain', terrain: word };
              if (withCoords) {
                item.x = x;
                item.y = y;
              }
              outArray.push(item);
              return;
            }
            return [word];
          }
          if (x < 0 || y < 0 || x > 49 || y > 49) {
            throw new Error('look coords are out of bounds');
          }
          const typeResult = lookSpatial[typeName][x * 50 + y];
          if (typeResult) {
            if (outArray) {
              typeResult.forEach((i) => {
                item = { type: typeName };
                item[typeName] = i;
                if (withCoords) {
                  item.x = x;
                  item.y = y;
                }
                outArray.push(item);
              });
              return;
            }
            return _.clone(typeResult);
          }
          return [];
        }

        // 官方 rooms.js:709-764 _lookAreaMixedRegister 逐字：寄存器键数 < 面积 →
        // 按对象（checkInside 官方运算优先级原样）；否则逐格 spatial（asArray 带
        // withCoords）。未知 type 在键数比较处 keys.length 自然 TypeError——官方
        // lookForAtArea 无 ERR_INVALID_ARGS 门（与 lookForAt 不对称，原样保留）
        function _lookAreaMixedRegister(type, top, left, bottom, right, withType, asArray, result) {
          const typeRegister = lookReg[type];
          const keys = typeRegister && Object.keys(typeRegister);
          if (type != 'terrain' && keys.length < (bottom - top + 1) * (right - left + 1)) {
            const checkInside = (i) => {
              return (
                (((!i.pos && i.roomName == name) || (i.pos && i.pos.roomName == name)) &&
                  i.pos &&
                  i.pos.y >= top &&
                  i.pos.y <= bottom &&
                  i.pos.x >= left &&
                  i.pos.x <= right) ||
                (!i.pos && i.y >= top && i.y <= bottom && i.x >= left && i.x <= right)
              );
            };
            let item;
            keys.forEach((key) => {
              const obj = typeRegister[key];
              if (checkInside(obj)) {
                if (withType) {
                  item = { type: type };
                  item[type] = obj;
                  if (asArray) {
                    result.push({
                      x: obj.x || obj.pos.x,
                      y: obj.y || obj.pos.y,
                      type: type,
                      [type]: obj,
                    });
                  } else {
                    result[obj.y || obj.pos.y][obj.x || obj.pos.x].push(item);
                  }
                } else {
                  if (asArray) {
                    result.push({ x: obj.x || obj.pos.x, y: obj.y || obj.pos.y, [type]: obj });
                  } else {
                    result[obj.y || obj.pos.y][obj.x || obj.pos.x] =
                      result[obj.y || obj.pos.y][obj.x || obj.pos.x] || [];
                    result[obj.y || obj.pos.y][obj.x || obj.pos.x].push(obj);
                  }
                }
              }
            });
          } else {
            for (let y = top; y <= bottom; y++) {
              for (let x = left; x <= right; x++) {
                if (asArray) {
                  _lookSpatialRegister(type, x, y, result, true);
                } else {
                  if (result[y][x]) {
                    _lookSpatialRegister(type, x, y, result[y][x]);
                  } else {
                    result[y][x] = _lookSpatialRegister(type, x, y, undefined);
                  }
                }
              }
            }
          }
        }

        // 官方 rooms.js:768-796 lookAt 逐字：官方固定序 14 类型（creep→energy→
        // resource→source→mineral→deposit→structure→flag→constructionSite→
        // terrain 第 10 位→nuke→tombstone→ruin→powerCreep）；Mod 类型键（带
        // 冒号）按键序追加在内建固定序之后，官方 customObjectPrototypes 协议
        // 不接入
        room.lookAt = function (firstArg, secondArg) {
          const xy = gw.fetchXYArguments(firstArg, secondArg);
          const x = xy[0],
            y = xy[1];
          const result = [];
          _lookSpatialRegister('creep', x, y, result);
          _lookSpatialRegister('energy', x, y, result);
          _lookSpatialRegister('resource', x, y, result);
          _lookSpatialRegister('source', x, y, result);
          _lookSpatialRegister('mineral', x, y, result);
          _lookSpatialRegister('deposit', x, y, result);
          _lookSpatialRegister('structure', x, y, result);
          _lookSpatialRegister('flag', x, y, result);
          _lookSpatialRegister('constructionSite', x, y, result);
          _lookSpatialRegister('terrain', x, y, result);
          _lookSpatialRegister('nuke', x, y, result);
          _lookSpatialRegister('tombstone', x, y, result);
          _lookSpatialRegister('ruin', x, y, result);
          _lookSpatialRegister('powerCreep', x, y, result);
          for (let m = 0; m < modLookTypes.length; m++) {
            _lookSpatialRegister(modLookTypes[m], x, y, result);
          }
          return result;
        };
        // 官方 rooms.js:798-806 lookForAt 逐字：未知类型（非 terrain 且不在
        // spatial 寄存器键集）返 ERR_INVALID_ARGS；terrain 走 [单词]
        room.lookForAt = function (type, firstArg, secondArg) {
          const xy = gw.fetchXYArguments(firstArg, secondArg);
          const x = xy[0],
            y = xy[1];
          if (type != 'terrain' && !(type in lookSpatial)) {
            return ERR_INVALID_ARGS;
          }
          return _lookSpatialRegister(type, x, y);
        };
        // 官方 rooms.js:808-845 lookAtArea 逐字：非 asArray 预填稠密格阵 []，
        // asArray 得 [{x,y,type,[type]:obj}]（terrain 项带坐标）
        room.lookAtArea = function (top, left, bottom, right, asArray) {
          const result = asArray ? [] : {};
          if (!asArray) {
            for (let y = top; y <= bottom; y++) {
              result[y] = {};
              for (let x = left; x <= right; x++) {
                result[y][x] = [];
              }
            }
          }
          _lookAreaMixedRegister('creep', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('energy', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('resource', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('source', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('mineral', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('deposit', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('structure', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('flag', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister(
            'constructionSite',
            top,
            left,
            bottom,
            right,
            true,
            asArray,
            result,
          );
          _lookAreaMixedRegister('terrain', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('nuke', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('tombstone', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('ruin', top, left, bottom, right, true, asArray, result);
          _lookAreaMixedRegister('powerCreep', top, left, bottom, right, true, asArray, result);
          for (let m = 0; m < modLookTypes.length; m++) {
            _lookAreaMixedRegister(
              modLookTypes[m],
              top,
              left,
              bottom,
              right,
              true,
              asArray,
              result,
            );
          }
          return result;
        };
        // 官方 rooms.js:847-860 lookForAtArea 逐字：非 asArray 只预填行对象（格
        // 惰性建——sparse map 原始对象无 type 包裹），asArray 得 [{x,y,[type]:obj}]
        room.lookForAtArea = function (type, top, left, bottom, right, asArray) {
          const result = asArray ? [] : {};
          if (!asArray) {
            for (let y = top; y <= bottom; y++) {
              result[y] = {};
            }
          }
          _lookAreaMixedRegister(type, top, left, bottom, right, false, asArray, result);
          return result;
        };
      },
    };
  };
})();
