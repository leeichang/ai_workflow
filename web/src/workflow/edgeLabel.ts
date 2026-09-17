/**
 * 邊標記的位置
 *
 * 取哪一端要看邊的方向，取錯會讓多個標記疊成一個：
 *   前進邊（condition 的是／否）由同一節點分岔出去，起點相同，
 *   必須取終點才分得開。
 *   退回邊則相反，多個節點都退回同一個修改節點，終點相同，
 *   要取起點。
 *
 * 直接解析 path 而非讓 layout 另外回傳座標：layout 已經算過一次，
 * 再存一份等價資料會有兩者不同步的風險。
 */

export interface LabelPoint {
  x: number
  y: number
}

const POINT_RE = /[ML] ([\d.]+) ([\d.]+)/g

export function edgeLabelPoint(path: string, isReturn: boolean): LabelPoint {
  const points = [...path.matchAll(POINT_RE)]
  if (points.length === 0) return { x: 0, y: 0 }

  const p = isReturn ? points[0] : points[points.length - 1]
  return { x: Number(p[1]), y: Number(p[2]) }
}

/** 轉成可直接綁定的 style。讓開連接線，避免蓋住插入點的加號。 */
export function edgeLabelStyle(path: string, isReturn: boolean): Record<string, string> {
  const { x, y } = edgeLabelPoint(path, isReturn)

  return isReturn
    ? { left: `${x - 56}px`, top: `${y - 22}px` }
    : { left: `${x + 16}px`, top: `${y - 24}px` }
}
