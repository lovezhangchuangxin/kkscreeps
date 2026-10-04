/* 当前查看者的市场查询缓存、单位转换和市场指令。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  const __Object = Object;
  const C_ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const C_ERR_FULL = globalThis.ERR_FULL;
  const C_OK = globalThis.OK;
  const C_MARKET_FEE = globalThis.MARKET_FEE;
  const C_MARKET_MAX_ORDERS = globalThis.MARKET_MAX_ORDERS;
  const C_ORDER_BUY = globalThis.ORDER_BUY;
  const C_ORDER_SELL = globalThis.ORDER_SELL;
  const C_INTERSHARD_RESOURCES = globalThis.INTERSHARD_RESOURCES;
  const C_RESOURCES_ALL = globalThis.RESOURCES_ALL;
  const C_RESOURCE_ENERGY_NAME = globalThis.RESOURCE_ENERGY;
  const C_ERR_NOT_OWNER = globalThis.ERR_NOT_OWNER;
  const C_ERR_TIRED_MKT = globalThis.ERR_TIRED;
  const C_ERR_NOT_ENOUGH_RESOURCES = globalThis.ERR_NOT_ENOUGH_RESOURCES;
  const { lodash } = globalThis.__kkp;

  // 市场数据来自当前查看者的世界数据，价格从毫 credit 转为 credit。
  // 订单、历史和交易结果在本 tick 首次查询时复制并缓存；订单动作记录 global 指令。
  function makeMarket() {
    const view = gw.world.market || {};
    const txView = view.transactions || {};
    let _incoming;
    let _outgoing;
    let _orders;
    // 官方 :205-233：transactionId=""+_id（投影 id 已为串）、sender/
    // recipient→{username} 或缺键（undefined 序列化丢键——两侧同形）
    function mapTransactions(list) {
      return (list || []).map((i) => {
        const out = {
          transactionId: '' + i.id,
          time: i.time,
          resourceType: i.resourceType,
          amount: i.amount,
          from: i.from,
          to: i.to,
        };
        if (i.description !== undefined) out.description = i.description;
        if (i.order !== undefined) out.order = i.order;
        if (i.sender !== undefined) out.sender = { username: i.sender };
        if (i.recipient !== undefined) out.recipient = { username: i.recipient };
        return out;
      });
    }
    // 订单簿原始投影（毫价——÷1000 在视图层，官方 _getOrders/orders getter
    ///deal API 的单位分工同位：deal 读 raw 毫价、视图读 credits）
    const ordersView = view.orders || {};
    const myOrdersRaw = view.my_orders || [];
    const historyView = view.history || { all: [] };
    // 官方 make 闭包缓存（market.js:10-11——每 tick 对象重建=每 tick 首读
    // 快照；P6 惰性缓存同款纪律）
    const cachedOrders = {};
    const cachedHistory = {};
    let ordersCreatedDuringTick = 0;
    // 官方 :13-27 _getOrders：falsy→'all'；非法资源名 {} 不缓存；深拷贝+
    // price÷1000
    function _getOrders(resourceType) {
      if (!resourceType) resourceType = 'all';
      if (!cachedOrders[resourceType]) {
        if (resourceType !== 'all' && !lodash.contains(C_RESOURCES_ALL, resourceType)
          && !lodash.contains(C_INTERSHARD_RESOURCES, resourceType)) {
          return {};
        }
        const raw = ordersView[resourceType] || {};
        const copy = __jsonParse(__jsonStringify(raw));
        for (const i in copy) {
          copy[i].price /= 1000;
        }
        cachedOrders[resourceType] = copy;
      }
      return cachedOrders[resourceType];
    }
    // 官方 deal API 的 terminal 门（:126 `_.find(userObjects, {type:'terminal',
    // room})`——自家 terminal；room.terminal 快捷面首个【同房多 terminal 官方
    // any() 任一、我方首个+my 过滤，混主夹具态登记】）
    function myTerminalIn(roomName) {
      const room = Game.rooms[roomName];
      return room && room.terminal && room.terminal.my ? gw.rawData(room.terminal.id) : null;
    }
    const market = {
      // 官方 :31-34：calcRoomsDistance(continuous) + calcTerminalEnergyCost
      //（utils.js:644-659）=ceil(amount×(1−e^(−range/30)))。**不环绕**：
      // 官方 MMO 经 getWorldSize 环绕取 min(size−d,d)；我方单机世界 plain
      // Chebyshev（与 Rust settle/market.rs calc_rooms_distance 同判登记，
      // liveWorldSize 能力键 false）。畸形房名：gw.roomNameToXY 是官方
      // utils.js:374-390 的宽松逐字移植——畸形名产 NaN 不抛错 → 距离/成本
      // NaN（number 返回，两侧同函数同形）
      calcTransactionCost(amount, roomName1, roomName2) {
        const xy1 = gw.roomNameToXY(roomName1);
        const xy2 = gw.roomNameToXY(roomName2);
        const dx = Math.abs(xy2[0] - xy1[0]);
        const dy = Math.abs(xy2[1] - xy1[1]);
        return Math.ceil(amount * (1 - Math.exp(-Math.max(dx, dy) / 30)));
      },
      // 官方 :36-39：_getOrders 快照 + lodash.filter（filter 缺省=全量）
      getAllOrders(filter) {
        const orders = _getOrders(filter && filter.resourceType);
        return lodash.filter(orders, filter);
      },
      // 官方 :41-53：非法资源名 {}；'all'→[]（driver 恒空初始化）；
      // 合法资源无数据 {} **对象非数组**（MARKET-QUERY-006 官方同红的根源）
      getHistory(resourceType) {
        if (!resourceType) resourceType = 'all';
        if (!cachedHistory[resourceType]) {
          if (resourceType !== 'all' && !lodash.contains(C_RESOURCES_ALL, resourceType)
            && !lodash.contains(C_INTERSHARD_RESOURCES, resourceType)) {
            return {};
          }
          cachedHistory[resourceType] = __jsonParse(__jsonStringify(historyView[resourceType] || {}));
        }
        return cachedHistory[resourceType];
      },
      // 官方 :55-66：先自己的（this.orders 已 ÷1000）→ 公共 active 池（÷1000）
      //→ null。this.orders 触发 orders getter 惰性缓存（官方同形）
      getOrderById(id) {
        if (this.orders[id]) {
          return __jsonParse(__jsonStringify(this.orders[id]));
        }
        const order = ordersView.all[id];
        if (order) {
          const result = __jsonParse(__jsonStringify(order));
          result.price /= 1000;
          return result;
        }
        return null;
      },
      // 官方 :68-98：对象/参数双形 → 资源门 → type 门 → parseFloat/parseInt →
      // 正数门 → fee>credits 门（credits 单位）→ terminal 门（intershard 免）
      //→ 数量门（size(orders)+本 tick 已建 ≥300）→ pushByName 无 maxLen
      createOrder(type, resourceType, price, totalAmount, roomName) {
        if (typeof type === 'object' && type !== null) {
          ({ type, resourceType, price, totalAmount, roomName } = type);
        }
        if (!lodash.contains(C_RESOURCES_ALL, resourceType)
          && !lodash.contains(C_INTERSHARD_RESOURCES, resourceType)) {
          return C_ERR_INVALID_ARGS;
        }
        if (type !== C_ORDER_BUY && type !== C_ORDER_SELL) {
          return C_ERR_INVALID_ARGS;
        }
        price = parseFloat(price);
        totalAmount = parseInt(totalAmount);
        if (!price || price <= 0 || !totalAmount) {
          return C_ERR_INVALID_ARGS;
        }
        if (price * totalAmount * C_MARKET_FEE > this.credits) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        if (!lodash.contains(C_INTERSHARD_RESOURCES, resourceType)
          && (!roomName || !myTerminalIn(roomName))) {
          return C_ERR_NOT_OWNER;
        }
        if (lodash.size(this.orders) + ordersCreatedDuringTick >= C_MARKET_MAX_ORDERS) {
          return C_ERR_FULL;
        }
        ordersCreatedDuringTick++;
        gw.intents.pushByName('global', 'createOrder', {
          type, resourceType, price, totalAmount, roomName,
        });
        return C_OK;
      },
      // 官方 :100-106：自己的订单门 → pushByName(50)（返回值忽略恒 OK）
      cancelOrder(orderId) {
        if (!this.orders[orderId]) {
          return C_ERR_INVALID_ARGS;
        }
        gw.intents.pushByName('global', 'cancelOrder', { orderId }, 50);
        return C_OK;
      },
      // 官方 :108-153：公共 active 池 **raw 毫价**；intershard 资源面门 /
      // 自家 terminal 门（transferCost=calcTransactionCost）+ 能量面门 +
      // 冷却门 + BUY 资源面门 + SELL money 门（毫×量 vs 毫）→ pushByName(10)
      // 满 ERR_FULL
      deal(orderId, amount, targetRoomName) {
        const order = ordersView.all[orderId];
        if (!order) {
          return C_ERR_INVALID_ARGS;
        }
        amount = parseInt(amount);
        if (!amount || amount < 0) {
          return C_ERR_INVALID_ARGS;
        }
        if (lodash.contains(C_INTERSHARD_RESOURCES, order.resourceType)) {
          if (order.type === C_ORDER_BUY
            && ((gw.world.market && gw.world.market.resources
              ? gw.world.market.resources[order.resourceType] : 0) || 0) < amount) {
            return C_ERR_NOT_ENOUGH_RESOURCES;
          }
        } else {
          if (!targetRoomName) {
            return C_ERR_INVALID_ARGS;
          }
          const t = myTerminalIn(targetRoomName);
          const transferCost = this.calcTransactionCost(amount, targetRoomName, order.roomName);
          if (!t) {
            return C_ERR_NOT_OWNER;
          }
          // 此检查把缺失 energy 当作 0；上游 engine/src/game/market.js 的 deal
          // 直接比较 store.energy，缺失时 undefined < cost 为 false，返回码可能不同。
          if (!t.store || (t.store[C_RESOURCE_ENERGY_NAME] || 0) < transferCost) {
            return C_ERR_NOT_ENOUGH_RESOURCES;
          }
          if (t.cooldownTime > gw.world.time) {
            return C_ERR_TIRED_MKT;
          }
          if (order.type === C_ORDER_BUY) {
            if ((order.resourceType !== C_RESOURCE_ENERGY_NAME
              && (!t.store[order.resourceType] || t.store[order.resourceType] < amount))
              || (order.resourceType === C_RESOURCE_ENERGY_NAME
                && t.store[C_RESOURCE_ENERGY_NAME] < amount + transferCost)) {
              return C_ERR_NOT_ENOUGH_RESOURCES;
            }
          }
        }
        if (order.type === C_ORDER_SELL && (view.money_milli || 0) < amount * order.price) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        if (!gw.intents.pushByName('global', 'deal', { orderId, targetRoomName, amount }, 10)) {
          return C_ERR_FULL;
        }
        return C_OK;
      },
      // 官方 :155-172：自己的订单 + parseFloat + 提价差额费（credits 单位，
      // order.price 为视图 credits 价）→ pushByName(50)
      changeOrderPrice(orderId, newPrice) {
        const order = this.orders[orderId];
        if (!order) {
          return C_ERR_INVALID_ARGS;
        }
        newPrice = parseFloat(newPrice);
        if (!newPrice || newPrice <= 0) {
          return C_ERR_INVALID_ARGS;
        }
        if (newPrice > order.price
          && (newPrice - order.price) * order.remainingAmount * C_MARKET_FEE > this.credits) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        gw.intents.pushByName('global', 'changeOrderPrice', { orderId, newPrice }, 50);
        return C_OK;
      },
      // 官方 :174-191：自己的订单 + parseInt + fee 门 → pushByName(50)
      extendOrder(orderId, addAmount) {
        const order = this.orders[orderId];
        if (!order) {
          return C_ERR_INVALID_ARGS;
        }
        addAmount = parseInt(addAmount);
        if (!addAmount || addAmount <= 0) {
          return C_ERR_INVALID_ARGS;
        }
        if (order.price * addAmount * C_MARKET_FEE > this.credits) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        gw.intents.pushByName('global', 'extendOrder', { orderId, addAmount }, 50);
        return C_OK;
      },
    };
    __Object.defineProperties(market, {
      // 官方 :196-201：`(user.money||0)/1000`——毫 credits ÷1000（注入面给
      // 毫原值，除法在 getter=官方同位）
      credits: {
        enumerable: true,
        get() { return (view.money_milli || 0) / 1000; },
      },
      incomingTransactions: {
        enumerable: true,
        get() {
          if (!_incoming) _incoming = mapTransactions(txView.incoming);
          return _incoming;
        },
      },
      outgoingTransactions: {
        enumerable: true,
        get() {
          if (!_outgoing) _outgoing = mapTransactions(txView.outgoing);
          return _outgoing;
        },
      },
      // 官方 :235-249：myOrders map——id 串化（投影已串）、删 user、
      // price÷1000、indexBy('id')；惰性闭包缓存（官方 _orders）
      orders: {
        enumerable: true,
        get() {
          if (!_orders) {
            _orders = lodash.indexBy(myOrdersRaw.map((i) => {
              const o = __jsonParse(__jsonStringify(i));
              delete o.user;
              o.price /= 1000;
              return o;
            }), 'id');
          }
          return _orders;
        },
      },
    });
    return market;
  }

  globalThis.__kkp.makeMarket = makeMarket;
})();
