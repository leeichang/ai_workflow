"""
Temporal Worker

執行 Workflow 與 Activity。Rust 端只做控制面（啟動、Signal、Query），
實際執行在這裡。

用法：
    python-ai/.venv/bin/python python-ai/worker.py

環境變數：
    TEMPORAL_TARGET     預設 localhost:7233
    TEMPORAL_NAMESPACE  預設 default
    TEMPORAL_TASK_QUEUE 預設 workflow-platform
"""

from __future__ import annotations

import asyncio
import logging
import os
import sys
from pathlib import Path

from temporalio.client import Client
from temporalio.worker import Worker

sys.path.insert(0, str(Path(__file__).parent))

from activities.human_task import ALL_ACTIVITIES  # noqa: E402
from workflows.interop_probe import InteropProbe  # noqa: E402
from workflows.interpreter import DslInterpreter  # noqa: E402

TARGET = os.getenv("TEMPORAL_TARGET", "localhost:7233")
NAMESPACE = os.getenv("TEMPORAL_NAMESPACE", "default")
TASK_QUEUE = os.getenv("TEMPORAL_TASK_QUEUE", "workflow-platform")


async def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")

    client = await Client.connect(TARGET, namespace=NAMESPACE)
    logging.info("已連線 Temporal %s (namespace=%s)", TARGET, NAMESPACE)

    worker = Worker(
        client,
        task_queue=TASK_QUEUE,
        # InteropProbe 是互通測試的夾具，與業務無關但留著，
        # 讓 temporal-client 的整合測試不需另外起 Worker。
        workflows=[DslInterpreter, InteropProbe],
        activities=ALL_ACTIVITIES,
    )

    logging.info("Worker 啟動，task_queue=%s", TASK_QUEUE)
    await worker.run()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        logging.info("Worker 已停止")
