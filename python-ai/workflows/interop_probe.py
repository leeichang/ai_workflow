"""
跨語言互通探針

用途單一：驗證 Rust 端的 temporal-client 與 Python 的 temporalio
在 Payload 編碼上相容。這是決議 D-02 的風險點——Temporal 沒有官方
Rust SDK，兩端各自實作編碼，不相容時的症狀是 Workflow 收到 None
或型別錯誤，而不是明確的解碼失敗。

這不是業務流程，是測試夾具。正式的 DSL Interpreter 在
`workflows/interpreter.py`（任務 1.4）。

刻意涵蓋的型別：巢狀物件、陣列、null、布林、浮點數、非 ASCII 字串。
每一種在兩端的編碼路徑都不同，只測物件會漏掉 null 的特例
（Python 用 binary/null 而非 json/plain 的 "null"）。
"""

from __future__ import annotations

import asyncio
from datetime import timedelta
from typing import Any

from temporalio import workflow


@workflow.defn(name="InteropProbe")
class InteropProbe:
    """接收任意 JSON、回應 Signal 與 Query，供 Rust 端驗證編碼往返。"""

    def __init__(self) -> None:
        self._received: dict[str, Any] = {}
        self._signals: list[Any] = []
        self._done = False
        self._cancelled = False

    @workflow.run
    async def run(self, payload: Any) -> dict[str, Any]:
        self._received = payload

        # 等待 Rust 端送出 finish Signal。設上限避免測試失敗時
        # 留下永久執行的流程佔用 Temporal。
        #
        # 取消必須明確處理。Temporal 的取消是把 CancelledError 丟進
        # 正在等待的協程，不 catch 的話流程會以「失敗」結束而非
        # 「已取消」，而且不會執行任何清理。正式的 Interpreter 會在
        # 這裡取消待辦、寫稽核紀錄，因此探針也照同樣的形狀寫。
        try:
            await workflow.wait_condition(
                lambda: self._done, timeout=timedelta(minutes=5)
            )
        except asyncio.CancelledError:
            self._cancelled = True
            return {
                "echoed": self._received,
                "signals": self._signals,
                "signal_count": len(self._signals),
                "cancelled": True,
            }

        return {
            "echoed": self._received,
            "signals": self._signals,
            "signal_count": len(self._signals),
            "cancelled": False,
        }

    @workflow.signal(name="add_value")
    async def add_value(self, value: Any) -> None:
        """收下任意值。用來驗證 Rust 送出的 Signal 參數解得開。"""
        self._signals.append(value)

    @workflow.signal(name="finish")
    async def finish(self) -> None:
        self._done = True

    @workflow.query(name="current_state")
    def current_state(self) -> dict[str, Any]:
        """回傳目前狀態。用來驗證 Rust 端解得開 Query 結果。"""
        return {
            "received": self._received,
            "signals": self._signals,
            "signal_count": len(self._signals),
            "done": self._done,
        }

    @workflow.query(name="echo_arg")
    def echo_arg(self, arg: Any) -> Any:
        """原樣回傳參數。驗證 Query 的「送出」方向。"""
        return arg
