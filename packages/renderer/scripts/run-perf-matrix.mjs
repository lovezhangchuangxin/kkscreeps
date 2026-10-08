/** R7 sequential matrix. Keep one headed CDP page foreground throughout. */
import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
const out = resolve(process.argv[2] ?? '/tmp/r7-final-matrix');
mkdirSync(out, { recursive: true });
function run(name, flags) {
  const args = [
    'proxy',
    'node',
    'scripts/perf-cdp.mjs',
    '--reload',
    'true',
    '--out',
    `${out}/${name}.json`,
    ...flags.flatMap(([key, value]) => [`--${key}`, String(value)]),
  ];
  console.log(`START ${name}`);
  const result = spawnSync('rtk', args, {
    cwd: process.cwd(),
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (result.status !== 0) {
    console.error(result.stdout, result.stderr);
    process.exit(result.status ?? 1);
  }
  // The JSON file retains all counts; terminal progress stays compact.
  console.log(`DONE ${name}`);
}
for (const backend of ['webgpu', 'webgl']) {
  for (let round = 1; round <= 3; round++) {
    run(`${backend}-cold-r${round}`, [
      ['backend', backend],
      ['action', 'startup'],
      ['cold', true],
    ]);
    for (const [kind, rate, moving, visuals] of [
      ['f0', 4, false, true],
      ['f1-1x', 1, true, true],
      ['f1-4x', 4, true, true],
    ])
      run(`${backend}-${kind}-r${round}`, [
        ['backend', backend],
        ['action', 'raf'],
        ['samples', 1800],
        ['warmup', 120],
        ['rate', rate],
        ['moving', moving],
        ['visuals', visuals],
      ]);
    run(`${backend}-serial-r${round}`, [
      ['backend', backend],
      ['action', 'serial'],
      ['samples', 1800],
      ['warmup', 120],
    ]);
    if (backend === 'webgpu')
      run(`${backend}-timestamps-r${round}`, [
        ['backend', backend],
        ['action', 'timestamps'],
        ['gpu', 'timestamp'],
        ['samples', 1800],
        ['warmup', 120],
      ]);
  }
  run(`${backend}-f3`, [
    ['backend', backend],
    ['action', 'raf'],
    ['samples', 1800],
    ['warmup', 120],
    ['rate', 4],
    ['visuals', false],
  ]);
  run(`${backend}-profile`, [
    ['backend', backend],
    ['action', 'raf'],
    ['profile', 1],
    ['samples', 1800],
    ['warmup', 120],
    ['rate', 4],
  ]);
  for (const [kind, rekey, terrain] of [
    ['same-id', false, false],
    ['rekey', true, false],
    ['terrain', false, true],
  ]) {
    run(`${backend}-snapshot-${kind}`, [
      ['backend', backend],
      ['action', 'snapshots'],
      ['samples', 120],
      ['rekey', rekey],
      ['terrain', terrain],
    ]);
    run(`${backend}-snapshot-${kind}-profile`, [
      ['backend', backend],
      ['action', 'snapshots'],
      ['profile', 1],
      ['samples', 30],
      ['rekey', rekey],
      ['terrain', terrain],
    ]);
  }
  for (const action of ['hover', 'hoverMove', 'ramparts', 'recovery', 'cropParity'])
    run(`${backend}-${action}`, [
      ['backend', backend],
      ['action', action],
    ]);
  // Main standard WebGPU chain is already archived from the same source build.
  for (const kind of backend === 'webgpu' ? ['pressure'] : ['standard', 'pressure'])
    run(`${backend}-soak-${kind}`, [
      ['backend', backend],
      ['action', 'soak'],
      ['kind', kind],
      ['ticks', 10000],
      ['warmup', 1000],
    ]);
}
