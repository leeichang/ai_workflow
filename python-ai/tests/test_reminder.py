"""
逾時前提醒測試

先前的行為是時間到才動作，事前完全沒有提示——
使用者被 AUTO_REJECT 時不知道發生了什麼事。

設定在節點層（2026-09-17 定案）：

  timeout:
    after: P3D
    policy: AUTO_REJECT
    remind_at: [P2D, P1D, PT12H, PT6H]

remind_at 的每個值是「距離到期還有多久」，不是「從現在起算多久」。
P2D 代表到期前兩天發一次。

提醒不影響逾時策略本身——發完提醒仍然繼續等，
時間到才依 policy 處理。
"""

from __future__ import annotations

import uuid

import pytest
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker

from dsl.models import InstanceStatus
from workflows.interpreter import DslInterpreter

from tests.test_timeout import ACTIVITIES, settle
from tests.test_interpreter import STATE, make_input, reset_state

pytestmark = pytest.mark.asyncio


@pytest.fixture(scope="module")
async def env():
    async with await WorkflowEnvironment.start_time_skipping() as e:
        yield e


def dsl_with_reminders(remind_at: list[str], after: str = "P3D") -> dict:
    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {
                "id": "approve",
                "type": "human_approval",
                "label": "主管簽核",
                "timeout": {
                    "after": after,
                    "policy": "AUTO_REJECT",
                    "remind_at": remind_at,
                },
                "on_reject": {"action": "end", "result": "rejected"},
            },
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [["start", "approve"], ["approve", "end"]],
    }


async def run(env: WorkflowEnvironment, dsl: dict):
    task_queue = f"rm-{uuid.uuid4()}"
    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[DslInterpreter],
        activities=ACTIVITIES,
    ):
        handle = await env.client.start_workflow(
            DslInterpreter.run,
            make_input(dsl),
            id=f"wf-{uuid.uuid4()}",
            task_queue=task_queue,
        )
        return await handle.result()


def reminders() -> list[dict]:
    """取出提醒類的通知"""
    return [n for n in STATE["notifications"] if n.get("kind") == "reminder"]


class Test提醒發送:
    async def test_依設定的時間點各發一次(self, env):
        reset_state()
        await run(env, dsl_with_reminders(["P2D", "P1D", "PT6H"]))

        assert len(reminders()) == 3

    async def test_沒設_remind_at_時不發提醒(self, env):
        reset_state()
        dsl = dsl_with_reminders([])
        del dsl["nodes"][1]["timeout"]["remind_at"]
        await run(env, dsl)

        assert reminders() == []

    async def test_提醒不影響逾時策略(self, env):
        reset_state()
        result = await run(env, dsl_with_reminders(["P1D"]))

        # 發過提醒，時間到仍依 policy 退回
        assert len(reminders()) == 1
        assert result.status is InstanceStatus.REJECTED

    async def test_提醒帶剩餘時間(self, env):
        """使用者要知道還剩多久，不只是「快到期了」"""
        reset_state()
        await run(env, dsl_with_reminders(["P1D"]))

        r = reminders()[0]
        assert r.get("remaining") == "P1D"

    async def test_提醒發給該節點的簽核人(self, env):
        reset_state()
        await run(env, dsl_with_reminders(["P1D"]))

        r = reminders()[0]
        # 不是發給申請人，是發給等著簽的人
        assert "u1" in str(r.get("to"))

    async def test_超過期限的設定被忽略(self, env):
        """remind_at 比 after 還長的話，那個時間點已經過去了"""
        reset_state()
        # 期限 1 天，卻要求「到期前 3 天」提醒——那是流程開始前
        await run(env, dsl_with_reminders(["P3D", "PT6H"], after="P1D"))

        # 只有 PT6H 有效
        assert len(reminders()) == 1


class Test提醒與簽核:
    async def test_簽完之後不再發提醒(self, env):
        reset_state()
        dsl = dsl_with_reminders(["P2D", "P1D", "PT6H"])

        task_queue = f"rm-{uuid.uuid4()}"
        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[DslInterpreter],
            activities=ACTIVITIES,
        ):
            handle = await env.client.start_workflow(
                DslInterpreter.run,
                make_input(dsl),
                id=f"wf-{uuid.uuid4()}",
                task_queue=task_queue,
            )
            from dsl.models import TaskCompleted

            # 第一次提醒（到期前兩天）之後就簽掉
            await env.sleep(60 * 60 * 25)  # 25 小時，過了 P2D 那個點
            await handle.signal(
                DslInterpreter.task_completed,
                TaskCompleted(
                    task_id="task-approve-0",
                    node_id="approve",
                    participant_id="u1",
                    decision="APPROVE",
                ),
            )
            result = await handle.result()

        assert result.status is InstanceStatus.COMPLETED
        # 只發了第一次，後面兩次不該發
        assert len(reminders()) == 1


class Test提醒失敗:
    async def test_提醒失敗不中斷流程(self, env):
        """寄信失敗不該讓整張單卡住"""
        reset_state()
        STATE["notify_fails"] = True

        result = await run(env, dsl_with_reminders(["P1D"]))

        # 提醒發不出去，流程仍照常逾時退回
        assert result.status is InstanceStatus.REJECTED


class Test提醒的欄位轉發:
    """Activity 要把 kind 與 remaining 轉給 internal API

    漏傳的話提醒信會顯示成通用的「流程通知」，
    收信的人不知道是催簽核還是別的事。這是實測時發現的——
    信寄出了，但標題不對。
    """

    async def test_kind_與_remaining_有傳到(self, env):
        reset_state()
        await run(env, dsl_with_reminders(["P1D"]))

        r = reminders()[0]
        assert r.get("kind") == "reminder"
        assert r.get("remaining") == "P1D"
