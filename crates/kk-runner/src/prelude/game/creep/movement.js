/* Creep 移动参数、路径解码与 Memory._move 缓存；安装阶段由入口显式调用。
 * 常量与部件检查从入口传入，方法只记录移动指令，结算与仲裁仍由 Rust 负责。
 */
(() => {
  'use strict';
  globalThis.__kkp.createCreepMovementApi = function createCreepMovementApi() {
    const gw = globalThis.__kkp.gw;
    const _ = globalThis.__kkp.lodash;
    // room.js（加载序在本段之前）挂载的官方序列化器；捕获防玩家替换
    // globalThis.Room 后影响 moveTo 路径缓存。
    const roomSerializePath = globalThis.Room.serializePath;

    // 官方 utils.js:80-100 offsetsByDirection 表（deserializePath 的方向位移源）
    const OFFSETS_BY_DIRECTION = {
      1: [0, -1],
      2: [1, -1],
      3: [1, 0],
      4: [1, 1],
      5: [0, 1],
      6: [-1, 1],
      7: [-1, 0],
      8: [-1, -1],
    };

    /** 官方 `_.isObject`（creeps.js:172 的 moveTo opts 归位门）：对象与函数
     *  都算（null 除外）。 */
    function isObjectLike(v) {
      return v !== null && (typeof v === 'object' || typeof v === 'function');
    }

    /** 官方 utils.js:34-63 fetchXYArguments（moveTo 目标解析）：第二参是
     *  number → 数值形态；否则对象形态（RoomPosition 取自身、RoomObject 取
     *  .pos——两者都不是则 x/y 保持 undefined）。NaN 归 undefined。instanceof
     *  用 gw.RoomPosition（预置捕获——玩家换 globalThis.RoomPosition 不影响
     *  API 判定，与 C 表纪律一致）。 */
    function fetchXYArguments(firstArg, secondArg) {
      let x, y, roomName;
      if (secondArg === undefined || typeof secondArg !== 'number') {
        if (!isObjectLike(firstArg)) return [undefined, undefined, undefined];
        if (firstArg instanceof gw.RoomPosition) {
          x = firstArg.x;
          y = firstArg.y;
          roomName = firstArg.roomName;
        }
        if (firstArg.pos && firstArg.pos instanceof gw.RoomPosition) {
          x = firstArg.pos.x;
          y = firstArg.pos.y;
          roomName = firstArg.pos.roomName;
        }
      } else {
        x = firstArg;
        y = secondArg;
      }
      if (Number.isNaN(x)) x = undefined;
      if (Number.isNaN(y)) y = undefined;
      return [x, y, roomName];
    }

    /** 官方 utils.js:576-609 deserializePath：序列化串 "XXYYddd..." →
     *  [{x, y, dx, dy, direction}]（首项坐标 = 第一步的落点，x-dx/y-dy 即其
     *  起步格——moveByPath 的对象形态匹配面）。非串/坐标 NaN/非法方向位都抛
     *  官方同文案错误。 */
    function deserializePath(path) {
      if (typeof path !== 'string') {
        throw new Error('`path` is not a string');
      }
      const result = [];
      if (!path.length) return result;
      // 不传进制参数，保留 parseInt 对 0x 前缀等输入的解析规则。
      let x = parseInt(path.substring(0, 2));
      let y = parseInt(path.substring(2, 4));
      if (Number.isNaN(x) || Number.isNaN(y)) {
        throw new Error('`path` is not a valid serialized path string');
      }
      for (let i = 4; i < path.length; i++) {
        const direction = parseInt(path.charAt(i));
        const offs = OFFSETS_BY_DIRECTION[direction];
        if (!offs) {
          throw new Error('`path` is not a valid serialized path string');
        }
        const dx = offs[0],
          dy = offs[1];
        if (i > 4) {
          x += dx;
          y += dy;
        }
        result.push({ x, y, dx, dy, direction });
      }
      return result;
    }

    function installPathCodec() {
      gw.deserializePath = deserializePath;
      // 官方 rooms.js:532-534：deserializePath 是 Room 静态；入口在原阶段安装。
      globalThis.Room.deserializePath = deserializePath;
    }

    function installMethods({ C, countActiveParts, assertTargetObject }) {
      /** Creep.move：官方 creeps.js:126-156——重预检（fatigue/bodypart/方向）后记
       *  intent 返回 OK；朝相邻 creep 形态（target 为 Creep 实例）同官方 :132-141，
       *  结构上跳过疲劳/MOVE 门（拉拽通道）。疲劳与 MOVE 门读计划派生常量
       *  （规则 builtin:creep.move.*，默认 0/1 即官方行为；与结算仲裁原生
       *  准入预填同源）。官方 :130-133 读 this.my/this.spawning（getter 通道）；
       *  :145 的 fatigue 是 data(this.id) 显式回查（新鲜读绕缓存——跨 tick
       *  旧壳按当前值判 ERR_TIRED）。 */
      globalThis.Creep.prototype.move = function move(target) {
        if (!this.my) return C.ERR_NOT_OWNER;
        if (this.spawning) return C.ERR_BUSY;
        if (target && target instanceof globalThis.Creep) {
          if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
          gw.intents.set(this.id, 'move', { id: target.id });
          return C.OK;
        }
        if (gw.rawData(this.id).fatigue > C.MOVE_FATIGUE_LIMIT) return C.ERR_TIRED;
        if (countActiveParts(this.body, C.MOVE) < C.MOVE_MIN_ACTIVE_MOVES) {
          return C.ERR_NO_BODYPART;
        }
        const direction = +target;
        if (!direction || direction < 1 || direction > 8) return C.ERR_INVALID_ARGS;
        gw.intents.set(this.id, 'move', { direction });
        return C.OK;
      };

      /** 官方 moveTo 全流程（engine/src/game/creeps.js 的 moveTo）：预检（归属/
       * 孵化/疲劳+visualizePathStyle 豁免/MOVE 部件/目标解析）后消费 creep
       * memory 的 _move 路径缓存（reusePath 缺省 5、serializeMemory 缺省 true；
       * 过期或换房删除、目的地匹配时按当前位置截断后 moveByPath），
       * noPathFinding 出口，findPathTo 真路径（range 缺省 1）写回缓存并走首步。
       * 数值坐标形态沿用调用方 opts 引用（官方同形：缺省值写入可见）；对象
       * 目标形态浅拷贝 secondArg。parseInt 用全局（官方 realm 语义）。 */
      globalThis.Creep.prototype.moveTo = function moveTo(firstArg, secondArg, opts) {
        if (!this.my) return C.ERR_NOT_OWNER;
        if (this.spawning) return C.ERR_BUSY;
        if (isObjectLike(firstArg)) {
          opts = _.clone(secondArg);
        }
        opts = opts || {};
        if (gw.rawData(this.id).fatigue > C.MOVE_FATIGUE_LIMIT && !opts.visualizePathStyle) {
          return C.ERR_TIRED;
        }
        if (countActiveParts(this.body, C.MOVE) < C.MOVE_MIN_ACTIVE_MOVES) {
          return C.ERR_NO_BODYPART;
        }
        const [x, y, rawRoomName] = fetchXYArguments(firstArg, secondArg);
        const roomName = rawRoomName || this.pos.roomName;
        if (x === undefined || y === undefined) {
          // 官方 creeps.js:182-184 先过 register.assertTargetObject（game.js:122-126）：
          // plain object + 24 位字符串 id 视为 Memory 序列化对象，抛官方文案
          assertTargetObject(firstArg);
          return C.ERR_INVALID_TARGET;
        }
        const targetPos = new gw.RoomPosition(x, y, roomName);
        if (opts.reusePath === undefined) opts.reusePath = 5;
        if (opts.serializeMemory === undefined) opts.serializeMemory = true;
        // 官方 :196-198 绘图样式缺省（官方经 _.defaults 原地补全调用方对象）
        if (opts.visualizePathStyle) {
          _.defaults(opts.visualizePathStyle, {
            fill: 'transparent',
            stroke: '#fff',
            lineStyle: 'dashed',
            strokeWidth: 0.15,
            opacity: 0.1,
          });
        }
        if (x == this.pos.x && y == this.pos.y && roomName == this.pos.roomName) {
          return C.OK;
        }
        // 官方 :241-278 _move 缓存消费
        let visualized = false;
        if (opts.reusePath && this.memory && isObjectLike(this.memory) && this.memory._move) {
          const _move = this.memory._move;
          if (
            gw.world.time > _move.time + parseInt(opts.reusePath) ||
            _move.room != this.pos.roomName
          ) {
            delete this.memory._move;
          } else if (_move.dest.room == roomName && _move.dest.x == x && _move.dest.y == y) {
            let path = _.isString(_move.path) ? deserializePath(_move.path) : _move.path;
            const idx = _.findIndex(path, { x: this.pos.x, y: this.pos.y });
            if (idx !== -1) {
              path.splice(0, idx + 1);
              _move.path = opts.serializeMemory ? roomSerializePath(path) : path;
            }
            if (path.length === 0) {
              return this.pos.isNearTo(targetPos) ? C.OK : C.ERR_NO_PATH;
            }
            if (opts.visualizePathStyle) {
              this.room.visual.poly(path, opts.visualizePathStyle);
              visualized = true;
            }
            const result = this.moveByPath(path);
            if (result === C.OK) {
              return C.OK;
            }
          }
        }
        if (opts.noPathFinding) {
          return C.ERR_NOT_FOUND;
        }
        const path = this.pos.findPathTo(targetPos, opts);
        // 官方 :285-293 缓存写回（serializeMemory 缺省序列化串驻留 memory）
        if (opts.reusePath && this.memory && isObjectLike(this.memory)) {
          this.memory._move = {
            dest: { x, y, room: roomName },
            time: gw.world.time,
            path: opts.serializeMemory ? roomSerializePath(path) : _.clone(path),
            room: this.pos.roomName,
          };
        }
        if (path.length === 0) {
          return C.ERR_NO_PATH;
        }
        // 官方 :296-298：缓存臂已绘制时抑制重寻路径的二次绘制
        if (opts.visualizePathStyle && !visualized) {
          this.room.visual.poly(path, opts.visualizePathStyle);
        }
        return this.move(path[0].direction);
      };

      /** Creep.moveByPath：官方 creeps.js:305-327——自身无任何预检（my/spawning/
       *  fatigue/MOVE 全部经委托的 this.move 生效）。RoomPosition 数组形态：当前
       *  位置在路径中找 idx（找不到则须紧邻首步，否则 ERR_NOT_FOUND），取下一
       *  步方向 move；序列化串形态先 deserializePath（官方 utils.js:576-609，
       *  非法串抛错）；findPathTo 风格对象数组按 x-dx/y-dy 匹配当前格取方向。 */
      globalThis.Creep.prototype.moveByPath = function moveByPath(path) {
        if (Array.isArray(path) && path.length > 0 && path[0] instanceof gw.RoomPosition) {
          let idx = -1;
          for (let i = 0; i < path.length; i++) {
            if (path[i].isEqualTo(this.pos)) {
              idx = i;
              break;
            }
          }
          if (idx === -1 && !path[0].isNearTo(this.pos)) {
            return C.ERR_NOT_FOUND;
          }
          idx += 1;
          if (idx >= path.length) return C.ERR_NOT_FOUND;
          return this.move(this.pos.getDirectionTo(path[idx]));
        }
        if (typeof path === 'string') {
          path = deserializePath(path);
        }
        if (!Array.isArray(path)) return C.ERR_INVALID_ARGS;
        const cur = path.find((i) => i.x - i.dx == this.pos.x && i.y - i.dy == this.pos.y);
        if (!cur) return C.ERR_NOT_FOUND;
        return this.move(cur.direction);
      };
    }

    return { installPathCodec, installMethods };
  };
})();
