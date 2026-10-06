import assert from 'node:assert/strict';
import { test } from 'node:test';
import { locateBinary } from '../dist/rpc.js';

test('显式指定不存在的 CLI 必须失败，不能回退到旧构建', () => {
  const previous = process.env.KKSCREEPS_BIN;
  process.env.KKSCREEPS_BIN = '/kkscreeps-nonexistent-quality-probe/binary';
  try {
    assert.throws(() => locateBinary(), /指定的 kkscreeps 二进制不存在/);
  } finally {
    if (previous === undefined) delete process.env.KKSCREEPS_BIN;
    else process.env.KKSCREEPS_BIN = previous;
  }
});
