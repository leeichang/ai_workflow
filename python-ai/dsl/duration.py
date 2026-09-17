"""
ISO 8601 duration 解析

DSL 的 timeout.after 用 ISO 8601 表示，例如 P3D、PT8H、P1DT12H。
選 ISO 而非秒數：P3D 對設計者可讀，259200 不可讀。

不用 isodate 之類的套件：只需要支援 DSL schema 允許的子集，
多一個相依就多一份 Temporal 沙箱相容性的風險。

年與月刻意不支援。它們的長度不固定（閏年、大小月），
用在簽核期限上語意含糊——「一個月後」是 28 天還是 31 天？
需要長期限時用 P30D 明確表示。
"""

from __future__ import annotations

import re
from datetime import timedelta

_PATTERN = re.compile(
    r"^P"
    r"(?:(?P<weeks>\d+(?:\.\d+)?)W)?"
    r"(?:(?P<days>\d+(?:\.\d+)?)D)?"
    r"(?:T"
    r"(?:(?P<hours>\d+(?:\.\d+)?)H)?"
    r"(?:(?P<minutes>\d+(?:\.\d+)?)M)?"
    r"(?:(?P<seconds>\d+(?:\.\d+)?)S)?"
    r")?$"
)


def parse_iso_duration(text: str) -> timedelta | None:
    """
    解析 ISO 8601 duration

    無法解析時回 None 而非拋錯。逾時設定錯誤不該讓進行中的流程
    整個失敗——當成「沒有設逾時」繼續跑，比中斷簽核好。
    設計器端會在發布前擋下格式錯誤。
    """
    if not text or not isinstance(text, str):
        return None

    match = _PATTERN.match(text.strip())
    if not match:
        return None

    parts = {k: float(v) for k, v in match.groupdict().items() if v is not None}
    if not parts:
        # "P" 或 "PT" 本身符合正則但沒有任何數值
        return None

    return timedelta(
        weeks=parts.get("weeks", 0),
        days=parts.get("days", 0),
        hours=parts.get("hours", 0),
        minutes=parts.get("minutes", 0),
        seconds=parts.get("seconds", 0),
    )
