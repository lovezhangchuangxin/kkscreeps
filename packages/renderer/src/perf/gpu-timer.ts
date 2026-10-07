/** Optional native timestamps on the actual Pixi command encoders, perf entry only. */
type GPUObject = Record<string, any>;
export function enableTimestampFeature(): void {
  const gpu = (navigator as unknown as { gpu?: GPUObject }).gpu;
  if (!gpu) return;
  const requestAdapter = gpu.requestAdapter.bind(gpu);
  gpu.requestAdapter = async (options: unknown) => {
    const adapter = await requestAdapter(options);
    if (adapter?.features.has('timestamp-query')) {
      const requestDevice = adapter.requestDevice.bind(adapter);
      adapter.requestDevice = (descriptor: GPUObject = {}) => requestDevice({ ...descriptor, requiredFeatures: [...new Set([...(descriptor.requiredFeatures ?? []), 'timestamp-query'])] });
    }
    return adapter;
  };
}

export function makeTimestampTimer(device: GPUObject) {
  if (!device.features.has('timestamp-query')) return null;
  const querySet = device.createQuerySet({ type: 'timestamp', count: 512 });
  const resolveBuffer = device.createBuffer({ size: 4096, usage: 512 | 4 });
  const readBuffer = device.createBuffer({ size: 4096, usage: 1 | 8 });
  const createEncoder = device.createCommandEncoder.bind(device);
  let active = false, queries = 0;
  device.createCommandEncoder = (options: unknown) => {
    const encoder = createEncoder(options), beginPass = encoder.beginRenderPass.bind(encoder);
    encoder.beginRenderPass = (descriptor: GPUObject) => {
      if (!active) return beginPass(descriptor);
      if (queries + 2 > 512) throw new Error('GPU timestamp query capacity exceeded');
      const start = queries; queries += 2;
      return beginPass({ ...descriptor, timestampWrites: { querySet, beginningOfPassWriteIndex: start, endOfPassWriteIndex: start + 1 } });
    };
    return encoder;
  };
  return {
    begin() { queries = 0; active = true; },
    async finish() {
      active = false;
      if (!queries) throw new Error('No actual GPU render pass captured');
      const encoder = createEncoder(); encoder.resolveQuerySet(querySet, 0, queries, resolveBuffer, 0); encoder.copyBufferToBuffer(resolveBuffer, 0, readBuffer, 0, queries * 8);
      device.queue.submit([encoder.finish()]); await readBuffer.mapAsync(1);
      try {
        const values = new BigUint64Array(readBuffer.getMappedRange(0, queries * 8));
        const passMs = Array.from({ length: queries / 2 }, (_, p) => Number(values[p * 2 + 1]! - values[p * 2]!) / 1e6);
        const rawNs = Array.from(values, v => v.toString());
        const first = values.reduce((a, b) => a < b ? a : b), last = values.reduce((a, b) => a > b ? a : b);
        return { rawNs, passMs, gpuPassSumMs: passMs.reduce((a, b) => a + b, 0), gpuSpanMs: Number(last - first) / 1e6, passes: queries / 2 };
      } finally { readBuffer.unmap(); }
    },
    destroy() { device.createCommandEncoder = createEncoder; querySet.destroy(); resolveBuffer.destroy(); readBuffer.destroy(); },
  };
}
