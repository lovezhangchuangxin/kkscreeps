/**
 * 官方地形 path 合并算法移植（screeps/renderer engine/src/lib/pathHelper.js
 * 逐行对照，F2 方案 §2.4「terrain path 合并烘焙」）。
 *
 * 语义：把匹配地形码的格集合合并为一条连通轮廓 SVG path——相邻格共享边不
 * 画线，外凸直角以 `a 50 50` 半径 50（=半格）圆弧外凸，凹角按邻居/对角邻居
 * 决定方角或圆角。轮廓自 (x*cell, y*cell + cell/2) 起，顺时针。
 *
 * 与官方的差异（仅缓存键，不影响视觉）：官方以 md5(结构化数组 JSON) 做重建
 * 门控；此处用 FNV-1a over 匹配格列表——门控语义相同（hash 不变 → path:
 * null 跳过重建），哈希值本身不出现在任何视觉面。
 *
 * 移植保真说明：官方 array[x][y] 为列主序（x 首维）；terrain codes 为行主序
 * （y*50+x）——本函数负责换算。递归深度上界 = 连通区域尺寸（50×50 全墙
 * ~2500 帧），JS 栈安全。
 */

export interface MergeOptions {
  /** 格边长（世界像素/格，官方 demo=100）。 */
  cell: number;
  /** 对角格连通也视为相连（官方 swamp=false / wall=false / rampart=false，
   * 全为默认 false；参数保留以保移植完整）。 */
  diagonalConnect?: boolean;
}

export interface MergeResult {
  /** 匹配集的稳定哈希（重建门控键）。 */
  hash: string;
  /** 合并轮廓 path；null=匹配集与上次相同（跳过重建）；空集返回 null。 */
  path: string | null;
  /** 匹配格数量（0=空集）。 */
  count: number;
}

/** FNV-1a 32bit（确定性、无依赖；仅作缓存门控键）。 */
export function fnv1a(input: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

/**
 * @param codes 地形码（行主序 y*roomSize+x；wall=1/swamp=2/lava=4 位掩码）
 * @param match 格匹配谓词（如 c => (c & 1) !== 0 取墙）
 * @param prevHash 上次重建的 hash（门控；undefined=强制重建）
 */
export function mergedOutlinePath(
  codes: ArrayLike<number>,
  match: (code: number) => boolean,
  options: MergeOptions,
  prevHash?: string,
): MergeResult {
  const roomSize = Math.sqrt(codes.length);
  if (!Number.isInteger(roomSize)) throw new Error(`codes 长度须为平方数: ${codes.length}`);
  const { cell, diagonalConnect = false } = options;

  // 官方 array[x][y]：列主序布尔阵；visited 同形
  const filled = (x: number, y: number): boolean =>
    x >= 0 && y >= 0 && x < roomSize && y < roomSize && match(codes[y * roomSize + x]!) === true;
  const visited = new Uint8Array(roomSize * roomSize);
  const seen = (x: number, y: number): boolean => visited[y * roomSize + x] === 1;
  const markSeen = (x: number, y: number): void => {
    visited[y * roomSize + x] = 1;
  };

  // 哈希按扫描序收集匹配格（x,y 与码值）
  let hashInput = '';
  let count = 0;
  for (let x = 0; x < roomSize; x++) {
    for (let y = 0; y < roomSize; y++) {
      if (filled(x, y)) {
        hashInput += `${x},${y},${codes[y * roomSize + x]!};`;
        count++;
      }
    }
  }
  const hash = fnv1a(hashInput);
  if (count === 0) return { hash, path: null, count };
  if (prevHash === hash) return { hash, path: null, count };

  let path = '';
  const N = roomSize;
  const half = cell / 2;

  // ---- 官方 corner 函数（逐行对照 pathHelper.js；path 增量指令语义不变，
  // 绝对坐标换算为 cell 比例：官方 cell=100/half=50，这里 cell/half）----
  function topLeftDownR(_x: number, _y: number): void {
    if (_x > 0 && _y > 0 && !filled(_x - 1, _y)) {
      path += `a ${half} ${half} 0 0 0 ${-half} ${-half} h ${half} `;
    } else {
      path += `v ${-half} `;
    }
  }
  function topLeftUpR(_x: number, _y: number): void {
    if (_y > 0 && _x > 0 && !filled(_x, _y - 1)) {
      path += `v ${-half} a ${half} ${half} 0 0 0 ${half} ${half} `;
    } else {
      path += `h ${half} `;
    }
  }
  function topLeftR(_x: number, _y: number): void {
    if (_x === 0 || filled(_x - 1, _y) || _y === 0 || filled(_x, _y - 1)) {
      path += `v ${-half} h ${half} `;
    } else {
      path += `a ${half} ${half} 0 0 1 ${half} ${-half} `;
    }
  }
  function topRightUpR(_x: number, _y: number): void {
    if (_y > 0 && _x < N - 1 && !filled(_x, _y - 1)) {
      path += `a ${half} ${half} 0 0 0 ${half} ${-half} v ${half} `;
    } else {
      path += `h ${half} `;
    }
  }
  function topRightDownR(_x: number, _y: number): void {
    if (_x < N - 1 && _y > 0 && !filled(_x + 1, _y)) {
      path += `h ${half} a ${half} ${half} 0 0 0 ${-half} ${half} `;
    } else {
      path += `v ${half} `;
    }
  }
  function topRightR(_x: number, _y: number): void {
    if (_x === N - 1 || filled(_x + 1, _y) || _y === 0 || filled(_x, _y - 1)) {
      path += `h ${half} v ${half} `;
    } else {
      path += `a ${half} ${half} 0 0 1 ${half} ${half} `;
    }
  }
  function bottomRightR(_x: number, _y: number): void {
    if (
      _x === N - 1 ||
      filled(_x + 1, _y) ||
      _y === N - 1 ||
      filled(_x, _y + 1) ||
      (filled(_x + 1, _y + 1) && (filled(_x + 1, _y) || diagonalConnect))
    ) {
      path += `v ${half} h ${-half} `;
    } else {
      path += `a ${half} ${half} 0 0 1 ${-half} ${half} `;
    }
  }
  function bottomLeftR(_x: number, _y: number): void {
    if (
      _x === 0 ||
      filled(_x - 1, _y) ||
      _y === N - 1 ||
      filled(_x, _y + 1) ||
      (filled(_x - 1, _y + 1) && (filled(_x - 1, _y) || diagonalConnect))
    ) {
      path += `h ${-half} v ${-half} `;
    } else {
      path += `a ${half} ${half} 0 0 1 ${-half} ${-half} `;
    }
  }

  // 递归轮廓追踪（官方 recurs；dx=1 横向深入，否则纵向）
  function recurs(x: number, y: number, dx: number): void {
    if (seen(x, y)) {
      if (dx) {
        path += `v ${cell} `;
      } else {
        path += `h ${-cell} `;
      }
      return;
    }

    if (dx) {
      // top left
      if (
        x === 0 ||
        y === 0 ||
        (filled(x - 1, y - 1) && (diagonalConnect || filled(x - 1, y) || filled(x, y - 1)))
      ) {
        topLeftUpR(x, y);
      } else {
        path += `h ${half} `;
      }

      if (x < N - 1 && filled(x + 1, y)) {
        // top right
        if (
          x === N - 1 ||
          y === 0 ||
          (filled(x + 1, y - 1) && (diagonalConnect || filled(x + 1, y) || filled(x, y - 1)))
        ) {
          topRightUpR(x, y);
        } else {
          path += `h ${half} `;
        }

        recurs(x + 1, y, 1);

        path += `h ${-cell} `;
      } else {
        // top right
        if (
          x === N - 1 ||
          y === 0 ||
          (filled(x + 1, y - 1) && (diagonalConnect || filled(x + 1, y) || filled(x, y - 1)))
        ) {
          topRightUpR(x, y);
          topRightDownR(x, y);
        } else {
          topRightR(x, y);
        }

        // bottom right
        bottomRightR(x, y);

        path += `h ${-half} `;
      }
    } else {
      // top right
      if (
        x === N - 1 ||
        y === 0 ||
        (filled(x + 1, y - 1) && (diagonalConnect || filled(x + 1, y) || filled(x, y - 1)))
      ) {
        topRightDownR(x, y);
      } else {
        path += `v ${half} `;
      }

      if (y < N - 1 && filled(x, y + 1)) {
        path += `v ${half} `;

        recurs(x, y + 1, 0);

        path += `v ${-half} `;
      } else {
        bottomRightR(x, y);
        bottomLeftR(x, y);
      }

      if (x === 0 || y === 0 || filled(x - 1, y - 1)) {
        topLeftDownR(x, y);
      } else {
        path += `v ${-half} `;
      }
    }

    if (x < N && y < N) {
      markSeen(x, y);
    }
  }

  for (let x = 0; x < N; x += 1) {
    for (let y = 0; y < N; y += 1) {
      if (filled(x, y) && !seen(x, y)) {
        path += `M ${x * cell} ${y * cell + half} `;

        markSeen(x, y);

        let horizontal = 0;
        do {
          horizontal += 1;
        } while (x + horizontal < N && filled(x + horizontal, y));

        let vertical = 0;
        do {
          vertical += 1;
        } while (y + vertical < N && filled(x, y + vertical));

        if (vertical < horizontal) {
          if (
            x === 0 ||
            y === 0 ||
            (filled(x - 1, y - 1) && (diagonalConnect || filled(x - 1, y) || filled(x, y - 1)))
          ) {
            // top left
            topLeftDownR(x, y);
            topLeftUpR(x, y);
          } else {
            topLeftR(x, y);
          }

          if (x < N - 1 && filled(x + 1, y)) {
            // top right
            if (
              x === N - 1 ||
              y === 0 ||
              (filled(x + 1, y - 1) && (diagonalConnect || filled(x + 1, y) || filled(x, y - 1)))
            ) {
              topRightUpR(x, y);
            } else {
              path += `h ${half} `;
            }

            recurs(x + 1, y, 1);

            path += `h ${-half} `;
          } else {
            // top right
            if (
              x === N - 1 ||
              y === 0 ||
              (filled(x + 1, y - 1) && (diagonalConnect || filled(x + 1, y) || filled(x, y - 1)))
            ) {
              topRightUpR(x, y);
              topRightDownR(x, y);
            } else {
              topRightR(x, y);
            }

            bottomRightR(x, y);
          }

          bottomLeftR(x, y);
        } else {
          // top left
          if (
            x === 0 ||
            y === 0 ||
            (filled(x - 1, y - 1) && (diagonalConnect || filled(x - 1, y) || filled(x, y - 1)))
          ) {
            topLeftDownR(x, y);
            topLeftUpR(x, y);
          } else {
            topLeftR(x, y);
          }

          // top right
          if (
            x === N - 1 ||
            y === 0 ||
            (filled(x + 1, y - 1) && (diagonalConnect || filled(x + 1, y) || filled(x, y - 1)))
          ) {
            topRightUpR(x, y);
            topRightDownR(x, y);
          } else {
            topRightR(x, y);
          }

          if (y < N - 1 && filled(x, y + 1)) {
            path += `v ${half} `;
            recurs(x, y + 1, 0);
            path += `v ${-half} `;
          } else {
            bottomRightR(x, y);
            bottomLeftR(x, y);
          }
        }

        path += 'Z ';
      }
    }
  }

  return { hash, path, count };
}
