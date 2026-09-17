"""
逾時策略測試

用 time-skipping 環境：Workflow 等待時虛擬時鐘會快轉，
因此「等兩天」在測試裡是瞬間的事。

四種策略：
  WAIT          繼續等，不做事
  AUTO_APPROVE  視為通過，流程前進
  AUTO_REJECT   視為退回，走 on_reject
  ESCALATE      加簽給指定的人，繼續等

ESCALATE 的語意（2026-09-17 定案）：
  - 原本的待辦**保留**，兩邊都能簽（是「加簽」不是「換人」）
  - 加簽者**算進** join 的分母（與原簽核人地位相同）

單節點與 join 節點共用同一套邏輯。
"""

from __future__ import annotations

import uuid

import pytest
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker

from dsl.models import InstanceInput, InstanceStatus
from workflows.interpreter import DslInterpreter

from temporalio import activity

from tests.test_interpreter import STATE, make_input, reset_state
from tests.test_interpreter import (
    cancel_human_tasks,
    create_human_tasks,
    report_instance_finished,
    run_action,
    send_notification,
)


@activity.defn(name="resolve_participants")
async def resolve_participants(req: dict) -> list[dict]:
    """依 resolver 回傳不同的人

    ESCALATE 的去重邏輯會濾掉已在名單裡的人（同一人不該收兩張待辦），
    因此 mock 必須能依 spec 回不同的參與者，否則加簽永遠是空的。
    """
    role = (req.get("spec") or {}).get("value")
    if role in STATE.get("by_role", {}):
        return STATE["by_role"][role]
    return STATE["participants"]


ACTIVITIES = [
    resolve_participants,
    create_human_tasks,
    cancel_human_tasks,
    run_action,
    send_notification,
    report_instance_finished,
]

pytestmark = pytest.mark.asyncio


@pytest.fixture(scope="module")
async def env():
    """快轉時鐘的測試環境

    逾時測試不能用 start_local()：那會真的等兩天。
    """
    async with await WorkflowEnvironment.start_time_skipping() as e:
        yield e


async def settle(handle) -> None:
    """等被取消的流程收尾

    不等的話分支還在跑而測試環境已拆，會噴
    Not in workflow event loop。取消後流程以 CANCELLED 結束，
    也可能拋 CancelledError，兩者都是預期的。
    """
    try:
        await handle.result()
    except Exception:  # noqa: BLE001
        pass


async def run(env: WorkflowEnvironment, inp: InstanceInput):
    task_queue = f"to-{uuid.uuid4()}"
    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[DslInterpreter],
        activities=ACTIVITIES,
    ):
        handle = await env.client.start_workflow(
            DslInterpreter.run,
            inp,
            id=f"wf-{uuid.uuid4()}",
            task_queue=task_queue,
        )
        return await handle.result()


# ── 單節點逾時 ──────────────────────────────────────────


def single_node_dsl(timeout: dict) -> dict:
    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {
                "id": "approve",
                "type": "human_approval",
                "label": "主管簽核",
                "timeout": timeout,
                "on_reject": {"action": "end", "result": "rejected"},
            },
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [["start", "approve"], ["approve", "end"]],
    }


class Test單節點逾時:
    async def test_AUTO_APPROVE_視為通過(self, env):
        reset_state()
        dsl = single_node_dsl({"after": "P2D", "policy": "AUTO_APPROVE"})
        result = await run(env, make_input(dsl))

        # 沒有人簽，但逾時後視為通過
        assert result.status is InstanceStatus.COMPLETED

    async def test_AUTO_REJECT_視為退回(self, env):
        reset_state()
        dsl = single_node_dsl({"after": "P2D", "policy": "AUTO_REJECT"})
        result = await run(env, make_input(dsl))

        assert result.status is InstanceStatus.REJECTED

    async def test_AUTO_REJECT_走_on_reject_的_goto(self, env):
        reset_state()
        dsl = {
            "nodes": [
                {"id": "start", "type": "trigger"},
                {
                    "id": "approve",
                    "type": "human_approval",
                    "timeout": {"after": "P1D", "policy": "AUTO_REJECT"},
                    "on_reject": {"action": "goto", "node": "revise"},
                },
                {"id": "revise", "type": "human_task", "label": "業務修改"},
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [
                ["start", "approve"],
                ["approve", "end"],
                ["revise", "approve"],
            ],
        }
        # revise 也會逾時，否則測試會卡住等人簽
        dsl["nodes"][2]["timeout"] = {"after": "P1D", "policy": "AUTO_REJECT"}
        dsl["nodes"][2]["on_reject"] = {"action": "end", "result": "rejected"}

        result = await run(env, make_input(dsl))

        assert "revise" in result.path, f"應走到 revise：{result.path}"

    async def test_逾時後未處理的待辦被取消(self, env):
        reset_state()
        dsl = single_node_dsl({"after": "P2D", "policy": "AUTO_APPROVE"})
        await run(env, make_input(dsl))

        # 否則使用者的收件匣留下永遠點不動的項目
        assert "task-approve-0" in set(STATE["cancelled"])

    async def test_沒有_timeout_時不會逾時(self, env):
        """沒設 timeout 的節點應該無限等待，不該被誤判逾時"""
        reset_state()
        dsl = {
            "nodes": [
                {"id": "start", "type": "trigger"},
                {"id": "a", "type": "human_approval"},
                {
                    "id": "b",
                    "type": "human_approval",
                    "timeout": {"after": "PT1H", "policy": "AUTO_APPROVE"},
                },
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [["start", "b"], ["b", "a"], ["a", "end"]],
        }
        # b 逾時放行後停在 a（無 timeout），流程不會結束
        task_queue = f"to-{uuid.uuid4()}"
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
            # 快轉很久，a 仍不該自己完成
            await env.sleep(60 * 60 * 24 * 30)
            state = await handle.query(DslInterpreter.current_state)
            assert "a" in (state.active_nodes or []), "沒設 timeout 的節點不該逾時"
            await handle.cancel()
            await settle(handle)


class TestESCALATE:
    def dsl(self) -> dict:
        return {
            "nodes": [
                {"id": "start", "type": "trigger"},
                {
                    "id": "approve",
                    "type": "human_approval",
                    "label": "主管簽核",
                    "resolver": {"type": "role", "value": "manager"},
                    "timeout": {
                        "after": "P2D",
                        "policy": "ESCALATE",
                        "to": {"type": "role", "value": "cfo"},
                    },
                },
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [["start", "approve"], ["approve", "end"]],
        }

    def setup_roles(self) -> None:
        STATE["by_role"] = {
            "manager": [{"id": "u1", "display": "張經理", "kind": "internal"}],
            "cfo": [{"id": "u9", "display": "李協理", "kind": "internal"}],
        }

    async def test_逾時後建立加簽待辦(self, env):
        reset_state()
        self.setup_roles()
        task_queue = f"to-{uuid.uuid4()}"
        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[DslInterpreter],
            activities=ACTIVITIES,
        ):
            handle = await env.client.start_workflow(
                DslInterpreter.run,
                make_input(self.dsl()),
                id=f"wf-{uuid.uuid4()}",
                task_queue=task_queue,
            )
            await env.sleep(60 * 60 * 24 * 3)

            # 第二批待辦，node_id 相同但是加簽的
            created = STATE["created"]
            assert len(created) >= 2, f"應有加簽待辦：{created}"
            await handle.cancel()
            await settle(handle)

    async def test_原待辦保留_不取消(self, env):
        """加簽不是換人，原簽核人晚一點回來也能簽"""
        reset_state()
        self.setup_roles()
        task_queue = f"to-{uuid.uuid4()}"
        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[DslInterpreter],
            activities=ACTIVITIES,
        ):
            handle = await env.client.start_workflow(
                DslInterpreter.run,
                make_input(self.dsl()),
                id=f"wf-{uuid.uuid4()}",
                task_queue=task_queue,
            )
            await env.sleep(60 * 60 * 24 * 3)

            state = await handle.query(DslInterpreter.current_state)
            # 原待辦仍在 pending 清單裡
            assert "task-approve-0" in (state.pending_task_ids or [])
            await handle.cancel()
            await settle(handle)

    async def test_加簽者算進分母(self, env):
        """ALL（1 人）加簽一人後變成 2 人才過"""
        reset_state()
        self.setup_roles()
        dsl = self.dsl()
        dsl["nodes"][1]["join"] = {"completion": "ALL", "result": "ALL_SUCCESS"}

        task_queue = f"to-{uuid.uuid4()}"
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
            await env.sleep(60 * 60 * 24 * 3)

            # 只簽原本那一位，不該完成——加簽者也要簽
            from dsl.models import TaskCompleted

            await handle.signal(
                DslInterpreter.task_completed,
                TaskCompleted(
                    task_id="task-approve-0",
                    node_id="approve",
                    participant_id="u1",
                    decision="APPROVE",
                ),
            )
            await env.sleep(10)

            state = await handle.query(DslInterpreter.current_state)
            assert "approve" in (state.active_nodes or []), "加簽者未簽不該前進"
            await handle.cancel()
            await settle(handle)

    async def test_缺少_to_時明確失敗(self, env):
        reset_state()
        dsl = self.dsl()
        del dsl["nodes"][1]["timeout"]["to"]

        result = await run(env, make_input(dsl))
        assert result.status is InstanceStatus.FAILED
        assert "ESCALATE" in (result.error or "")


# ── join 節點逾時 ───────────────────────────────────────


def parallel_dsl(timeout: dict | None = None, join: dict | None = None) -> dict:
    merge: dict = {
        "id": "merge",
        "type": "join",
        "join": join or {"completion": "ALL", "result": "ALL_SUCCESS"},
    }
    if timeout is not None:
        merge["timeout"] = timeout

    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {"id": "split", "type": "parallel"},
            {"id": "a", "type": "human_approval", "label": "財務"},
            {"id": "b", "type": "human_approval", "label": "法務"},
            merge,
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [
            ["start", "split"],
            ["split", "a"],
            ["split", "b"],
            ["a", "merge"],
            ["b", "merge"],
            ["merge", "end"],
        ],
    }


class TestJoin逾時:
    async def test_AUTO_APPROVE_逾時後放行(self, env):
        """並簽時有人長期不簽，逾時放行而非永久卡住"""
        reset_state()
        dsl = parallel_dsl({"after": "P2D", "policy": "AUTO_APPROVE"})
        result = await run(env, make_input(dsl))

        assert result.status is InstanceStatus.COMPLETED

    async def test_AUTO_REJECT_逾時後退回(self, env):
        reset_state()
        dsl = parallel_dsl({"after": "P2D", "policy": "AUTO_REJECT"})
        result = await run(env, make_input(dsl))

        assert result.status is InstanceStatus.REJECTED

    async def test_逾時後未完成的分支待辦被取消(self, env):
        reset_state()
        dsl = parallel_dsl({"after": "P2D", "policy": "AUTO_APPROVE"})
        await run(env, make_input(dsl))

        cancelled = set(STATE["cancelled"])
        assert "task-a-0" in cancelled
        assert "task-b-0" in cancelled

    async def test_沒有_timeout_時無限等待(self, env):
        reset_state()
        dsl = parallel_dsl(None)

        task_queue = f"to-{uuid.uuid4()}"
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
            await env.sleep(60 * 60 * 24 * 30)

            state = await handle.query(DslInterpreter.current_state)
            # 兩個分支都還在等
            assert set(state.active_nodes or []) == {"a", "b"}
            await handle.cancel()
            await settle(handle)

    async def test_逾時前簽完不受影響(self, env):
        """逾時只是保險，正常簽完就正常走"""
        reset_state()
        dsl = parallel_dsl({"after": "P30D", "policy": "AUTO_REJECT"})

        task_queue = f"to-{uuid.uuid4()}"
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

            # 等兩個分支的待辦都建立好再送決策，
            # 太早送會被 node_id 檢查丟掉
            for _ in range(100):
                state = await handle.query(DslInterpreter.current_state)
                if set(state.active_nodes or []) == {"a", "b"}:
                    break
                await env.sleep(1)

            for node in ("a", "b"):
                await handle.signal(
                    DslInterpreter.task_completed,
                    TaskCompleted(
                        task_id=f"task-{node}-0",
                        node_id=node,
                        participant_id="u1",
                        decision="APPROVE",
                    ),
                )

            result = await handle.result()
            assert result.status is InstanceStatus.COMPLETED

    async def test_WAIT_繼續等不放行(self, env):
        """WAIT 的語意就是繼續等，不該有出口"""
        reset_state()
        dsl = parallel_dsl({"after": "PT1H", "policy": "WAIT"})

        task_queue = f"to-{uuid.uuid4()}"
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
            await env.sleep(60 * 60 * 24 * 7)

            state = await handle.query(DslInterpreter.current_state)
            assert set(state.active_nodes or []) == {"a", "b"}
            # 取消後等流程真的收尾，否則分支還在跑而測試環境已拆，
            # 會噴 Not in workflow event loop
            await handle.cancel()
            await settle(handle)

    async def test_join_不支援_ESCALATE_明確失敗(self, env):
        """分支各有自己的簽核人，加簽給誰沒有明確語意

        猜測一個對象比明確失敗更糟：使用者以為設好了，
        實際上加簽給了不預期的人。
        """
        reset_state()
        dsl = parallel_dsl(
            {
                "after": "P1D",
                "policy": "ESCALATE",
                "to": {"type": "role", "value": "cfo"},
            }
        )
        result = await run(env, make_input(dsl))

        assert result.status is InstanceStatus.FAILED
        assert "ESCALATE" in (result.error or "")
