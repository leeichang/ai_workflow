"""ISO 8601 duration 解析測試"""

from __future__ import annotations

from datetime import timedelta

import pytest

from dsl.duration import parse_iso_duration


class Test有效格式:
    @pytest.mark.parametrize(
        "text,expected",
        [
            ("P3D", timedelta(days=3)),
            ("P1W", timedelta(weeks=1)),
            ("PT8H", timedelta(hours=8)),
            ("PT30M", timedelta(minutes=30)),
            ("PT45S", timedelta(seconds=45)),
            ("P1DT12H", timedelta(days=1, hours=12)),
            ("P2DT3H30M", timedelta(days=2, hours=3, minutes=30)),
            ("PT1H30M45S", timedelta(hours=1, minutes=30, seconds=45)),
        ],
    )
    def test_解析正確(self, text, expected):
        assert parse_iso_duration(text) == expected

    def test_fixture_用到的值(self):
        # quotation_approval_v1.json 實際用的兩個值
        assert parse_iso_duration("P2D") == timedelta(days=2)
        assert parse_iso_duration("P7D") == timedelta(days=7)

    def test_前後空白會被忽略(self):
        assert parse_iso_duration("  P3D  ") == timedelta(days=3)

    def test_小數可接受(self):
        assert parse_iso_duration("PT1.5H") == timedelta(hours=1.5)


class Test無效格式:
    @pytest.mark.parametrize(
        "text",
        [
            "",
            "   ",
            "3D",  # 缺 P
            "P",  # 只有 P，沒有數值
            "PT",
            "P3X",  # 未知單位
            "3 days",
            "P-3D",  # 負值
            "abc",
        ],
    )
    def test_回_none_而非拋錯(self, text):
        # 逾時設定錯誤不該讓進行中的流程整個失敗。
        # 當成「沒設逾時」繼續跑，比中斷簽核好。
        assert parse_iso_duration(text) is None

    def test_非字串回_none(self):
        assert parse_iso_duration(None) is None
        assert parse_iso_duration(123) is None


class Test刻意不支援:
    @pytest.mark.parametrize("text", ["P1Y", "P1M", "P1Y2M"])
    def test_年月不支援(self, text):
        # 年與月的長度不固定（閏年、大小月），用在簽核期限上語意含糊：
        # 「一個月後」是 28 天還是 31 天？需要長期限時用 P30D 明確表示。
        assert parse_iso_duration(text) is None

    def test_月的_m_不會被誤認為分鐘(self):
        # P1M（一個月）與 PT1M（一分鐘）只差一個 T。
        # 若把前者解成一分鐘，三十天的期限會變成一分鐘後就逾時。
        assert parse_iso_duration("P1M") is None
        assert parse_iso_duration("PT1M") == timedelta(minutes=1)
