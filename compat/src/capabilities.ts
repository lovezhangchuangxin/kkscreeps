// 声明 screeps-ok 可以启用的可选测试组，字段含义来自其 src/adapter.ts。
// true 允许运行对应测试，不表示该组全部用例已通过；false 使 requires 检查跳过该组。
// 上游版本由 scripts/setup-screeps-ok.sh 的 SCREEPS_OK_PIN 固定。

export interface AdapterCapabilities {
  chemistry: boolean;
  powerCreeps: boolean;
  powerCreepAccountApi: boolean;
  powerEffects: boolean;
  powerSpawn: boolean;
  factory: boolean;
  terminal: boolean;
  marketBasics: boolean;
  market: boolean;
  terminalSend: boolean;
  observer: boolean;
  nuke: boolean;
  deposit: boolean;
  powerBank: boolean;
  terrain: boolean;
  roomStatus: boolean;
  portals: boolean;
  invaderCore: boolean;
  strongholdDeploy: boolean;
  invaderRaidSpawner: boolean;
  multiShard: boolean;
  interShardMemory: boolean;
  cpuShardLimits: boolean;
  liveWorldSize: boolean;
  actionLogCapture: boolean;
  randomInjection: boolean;
  deprecationNotices: boolean;
}

export const capabilities: AdapterCapabilities = {
  chemistry: true,
  powerCreeps: true,
  powerCreepAccountApi: true,
  powerEffects: true,
  powerSpawn: true,
  factory: true,
  terminal: true,
  marketBasics: true,
  market: true,
  terminalSend: true,
  observer: false,
  nuke: true,
  deposit: false,
  powerBank: false,
  terrain: true,
  roomStatus: false,
  portals: false,
  invaderCore: false,
  strongholdDeploy: false,
  invaderRaidSpawner: false,
  multiShard: false,
  interShardMemory: false,
  cpuShardLimits: false,
  liveWorldSize: false,
  actionLogCapture: false,
  randomInjection: false,
  deprecationNotices: false,
};
