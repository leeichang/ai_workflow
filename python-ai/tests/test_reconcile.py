"""
狀態一致性測試

Temporal 與 PostgreSQL 是兩份狀態。Temporal 知道流程結束了，
但收件匣與報表查的是 PostgreSQL——回報失敗的話，
單據會永遠顯示「進行中」。

先前的做法是重試 5 次後記 warning 放棄，留給對帳程序修正。
但那個對帳程序從未實作，而且「事後補」本來就比「當下寫成功」脆弱。

改為無限重試（2026-09-17 定案 D-06 修訂）：
資料庫寫不進去時流程停在這裡，而那本來就是系統無法運作的狀態。
在 Temporal UI 看得到卡住的流程，比資料靜默不一致好。
"""

from __future__ import annotations

import uuid

import pytest
from temporalio import activity
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker

from dsl.models import InstanceStatus
from workflows.interpreter import DslInterpreter

from tests.test_interpreter import (
    STATE,
    cancel_human_tasks,
    create_human_tasks,
    make_input,
    reset_state,
    resolve_participants,
    run_action,
    send_notification,
)

pytestmark = pytest.mark.asyncio


@activity.defn(name="report_instance_finished")
async def flaky_report(req: dict) -> None:
    """前 N 次失敗的回報

    模擬資料庫短暫斷線。STATE["report_fail_times"] 控制失敗次數。
    """
    STATE.setdefault("report_attempts", 0)
    STATE["report_attempts"] += 1

    if STATE["report_attempts"] <= STATE.get("report_fail_times", 0):
        raise RuntimeError(f"模擬資料庫斷線（第 {STATE['report_attempts']} 次）")

    STATE["finished"].append(req)


ACTIVITIES = [
    resolve_participants,
    create_human_tasks,
    cancel_human_tasks,
    run_action,
    send_notification,
    flaky_report,
]


@pytest.fixture(scope="module")
async def env():
    async with await WorkflowEnvironment.start_time_skipping() as e:
        yield e


def simple_dsl() -> dict:
    """會自動走完的流程，不需要人簽"""
    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [["start", "end"]],
    }


async def run(env: WorkflowEnvironment):
    task_queue = f"rc-{uuid.uuid4()}"
    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[DslInterpreter],
        activities=ACTIVITIES,
    ):
        handle = await env.client.start_workflow(
            DslInterpreter.run,
            make_input(simple_dsl()),
            id=f"wf-{uuid.uuid4()}",
            task_queue=task_queue,
        )
        return await handle.result()


class Test回報重試:
    async def test_一次成功時只呼叫一次(self, env):
        reset_state()
        STATE["report_fail_times"] = 0

        result = await run(env)

        assert result.status is InstanceStatus.COMPLETED
        assert STATE["report_attempts"] == 1
        assert len(STATE["finished"]) == 1

    async def test_失敗後重試直到成功(self, env):
        reset_state()
        # 失敗 8 次——超過先前 maximum_attempts=5 的上限
        STATE["report_fail_times"] = 8

        result = await run(env)

        assert result.status is InstanceStatus.COMPLETED
        # 前 8 次失敗，第 9 次成功
        assert STATE["report_attempts"] == 9
        assert len(STATE["finished"]) == 1

    async def test_重試期間狀態不會寫成錯的(self, env):
        """失敗的嘗試不該留下半套資料"""
        reset_state()
        STATE["report_fail_times"] = 3

        await run(env)

        # 只有成功那次寫入，不是四筆
        assert len(STATE["finished"]) == 1
        assert STATE["finished"][0]["status"] == "COMPLETED"

    async def test_回報的內容正確(self, env):
        reset_state()
        STATE["report_fail_times"] = 2

        result = await run(env)

        report = STATE["finished"][0]
        assert report["status"] == result.status.value
        assert report["instance_id"] is not None
        assert report["tenant_id"] is not None
