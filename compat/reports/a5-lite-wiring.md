# M2A-A5-lite 接线批（B3 接线 + upgradeController 补漏）

基线 main=e8445ae（A3c+B3+C3 已合）。本批是 A5 的先行子批：点亮钥匙优先。

## 内容

1. **Tombstone/Ruin 玩家类接线**（官方 tombstones.js:41-135 / ruins.js:38-70）：
   defineGameObjectProperties 字段表（deathTime/ticksToDecay 折叠值直读/store
   makeStore/carry 族）+ creep 代理对象（id/name/spawning=false/my/body(hits:0)/
   owner/ticksToLive/carryCapacity/carry/fatigue=0/hits=0/hitsMax/saying——
   官方 defineProperties 覆盖形态同构，Creep 原型链）+ structure 代理对象
   （user 有→OwnedStructure 形含 owner/my，无→Structure 形；structureType 读
   存照 type 键——官方 getter 写 o.structureType 但 DB 键在 structure.type，
   vanilla snapshots.ts:394 同构）；assemblers 透传壳
2. **room.getEventLog(raw)**（官方 rooms.js:573-583）：世界投影 rooms 条目注入
   events（B3 RoomState.events 落账，仅可见房——官方 roomEventLog 按可见房
   data.js:141/:219）；raw=true 经预置 stringify 返回字符串形态
3. **Structure.destroy() 换真**（官方 structures.js:72-86 三门：!room/
   controller 非我/房内敌方 creep·powerCreep——FIND 常量走冻结 C 表防判定门
   失明；room 级 intent 经 pushByName('room','destroyStructure',{roomName,id})
   数组通道——from_json "room" 分支同形）；**STRUCTURE_METHODS 桩列表移除
   destroy（坑⑥复发实证：实例级桩遮蔽基类真方法，症状假 OK 零 intent）**
4. **prelude room.find 补 118/119/122/123 四臂**（FIND_TOMBSTONES/POWER_CREEPS/
   DEPOSITS/RUINS——后两类无对象恒空，臂在保持与 Rust matches_find 同构）
5. **upgradeController 换真**（官方 creeps.js:922-964 十门——shardAccess 门
   hosted 面不可达登记跳过；执行器 GPT boost 批已完善）；transfer(控制器,
   能量) 重定向自此走真门（既有矩阵断言 -12=无 WORK 真门，正路径新测试
   redirect_upgrade_positive_impl）
6. **pushByName 补计费**（make.js:100-106 按元素 0.2——JS 显示面，Rust 结算
   面原有）

## 测试

- sandbox_tests +2：destroy_room_intent_channel（room 通道形态+计费）、
  redirect_upgrade_positive_impl（重定向+直调双 OK）
- embed 冒烟全链：destroy→intent→结算→废墟契约七键→玩家面 Ruin 实例/
  structure 代理/store/ticksToDecay + getEventLog 事件（event=2）；Tombstone
  类/ticksToDecay/store/creep 代理（instanceof/hitsMax/body）
- workspace 全绿 + fmt + clippy 零警告

## 发现与登记

- 坑⑥复发实证（桩覆盖序）——灰区登记
- shardAccess 门不可达——灰区登记
- pushByName 计费差（JS 显示面）——已补
