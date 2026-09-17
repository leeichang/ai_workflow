"""
DSL Interpreter 測試

用 Temporal 的測試環境，Activity 以 mock 取代。
測的是「圖走訪與決策邏輯」，不是 Activity 的實作。

time-skipping 環境會在 Workflow 等待時快轉虛擬時鐘，
因此逾時測試不需要真的等三天。
"""

from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any

import pytest
from temporalio import activity
from temporalio.client import Client, WorkflowFailureError
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker

from dsl.models import InstanceInput, InstanceStatus, TaskCompleted
from workflows.interpreter import DslInterpreter

FIXTURES = Path(__file__).resolve().parents[2] / "schemas" / "fixtures"

# ── Mock Activities ─────────────────────────────────────

# 測試間共享的狀態。每個測試開始前重設。
STATE: dict[str, Any] = {}


DEFAULT_PARTICIPANTS = [
    {"id": "u1", "display": "王經理", "role": "manager", "kind": "internal"}
]


def reset_state(participants: list[dict] | None = None) -> None:
    STATE.clear()
    # 用 is None 而非 or：空陣列是有意義的輸入（測「找不到參與者」），
    # 寫成 `participants or DEFAULT` 會讓空陣列落回預設值，
    # 於是該測試實際上測到了「有一個參與者」，永遠不會通過。
    STATE["participants"] = (
        DEFAULT_PARTICIPANTS if participants is None else participants
    )
    STATE["created"] = []
    STATE["cancelled"] = []
    STATE["actions"] = []
    STATE["notifications"] = []
    STATE["finished"] = []
    STATE["action_fails"] = set()


@activity.defn(name="resolve_participants")
async def resolve_participants(req: dict) -> list[dict]:
    return STATE["participants"]


@activity.defn(name="create_human_tasks")
async def create_human_tasks(req: dict) -> list[str]:
    ids = [f"task-{req['node_id']}-{i}" for i in range(len(req["participants"]))]
    STATE["created"].append({"node_id": req["node_id"], "task_ids": ids})
    return ids


@activity.defn(name="cancel_human_tasks")
async def cancel_human_tasks(req: dict) -> None:
    STATE["cancelled"].extend(req["task_ids"])


@activity.defn(name="run_action")
async def run_action(req: dict) -> dict:
    STATE["actions"].append(req)
    if req["node_id"] in STATE["action_fails"]:
        raise RuntimeError(f"模擬 {req['node_id']} 失敗")
    return {"business_object": {f"{req['node_id']}_done": True}}


@activity.defn(name="send_notification")
async def send_notification(req: dict) -> None:
    # 先記錄再決定要不要失敗：測試要能看到「嘗試發送過」
    STATE["notifications"].append(req)
    if STATE.get("notify_fails"):
        raise RuntimeError("模擬通知服務失敗")


@activity.defn(name="report_instance_finished")
async def report_instance_finished(req: dict) -> None:
    STATE["finished"].append(req)


ACTIVITIES = [
    resolve_participants,
    create_human_tasks,
    cancel_human_tasks,
    run_action,
    send_notification,
    report_instance_finished,
]


# ── 測試夾具 ────────────────────────────────────────────


@pytest.fixture(scope="module")
async def env():
    """本機測試環境，不快轉時鐘

    module 範圍共用同一個 Server。每個測試各起一個會讓整批
    測試從數秒變成數分鐘——啟動 Temporal Server 本身就要好幾秒。
    測試間以獨立的 task_queue 與 workflow_id 隔離，共用不會互相干擾。

    刻意不用 start_time_skipping()：它會在 Workflow 等待時快轉
    虛擬時鐘，而我們的測試需要輪詢 Query 等節點就緒再送 Signal。
    快轉會讓等待中的流程瞬間衝到逾時，測試全數失敗。

    逾時相關的測試另外用 time-skipping，見 test_timeout.py。
    """
    async with await WorkflowEnvironment.start_local() as e:
        yield e


def make_input(dsl: dict, business: dict | None = None) -> InstanceInput:
    return InstanceInput(
        tenant_id="11111111-1111-1111-1111-111111111111",
        instance_id=str(uuid.uuid4()),
        workflow_version_id=str(uuid.uuid4()),
        dsl=dsl,
        business_object=business or {},
    )


async def wait_for_node(handle, node_id: str, timeout: float = 10.0) -> None:
    """等流程走到指定節點

    Signal 必須在節點成為當前節點之後才送。太早送會被
    interpreter 的 node_id 檢查丟掉——那個檢查是對的
    （退回後重跑同一節點時要能分辨是哪一輪），
    但測試得配合實際的時序。
    """
    import asyncio

    deadline = asyncio.get_event_loop().time() + timeout
    while asyncio.get_event_loop().time() < deadline:
        state = await handle.query(DslInterpreter.current_state)
        if state.current_node == node_id and state.pending_task_ids:
            return
        await asyncio.sleep(0.05)

    raise AssertionError(f"等不到節點 {node_id} 出現待辦")


async def signal_at(handle, sig: TaskCompleted) -> None:
    """等節點就緒後送出決策"""
    await wait_for_node(handle, sig.node_id)
    await handle.signal(DslInterpreter.task_completed, sig)


async def run_workflow(env: WorkflowEnvironment, inp: InstanceInput, signals=None):
    """啟動流程，依序送出 signal，回傳結果"""
    task_queue = f"test-{uuid.uuid4()}"

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

        for sig in signals or []:
            await signal_at(handle, sig)

        return await handle.result()


def approve(node_id: str, index: int = 0, participant: str = "u1") -> TaskCompleted:
    return TaskCompleted(
        task_id=f"task-{node_id}-{index}",
        node_id=node_id,
        participant_id=participant,
        decision="APPROVE",
    )


def reject(node_id: str, index: int = 0, participant: str = "u1") -> TaskCompleted:
    return TaskCompleted(
        task_id=f"task-{node_id}-{index}",
        node_id=node_id,
        participant_id=participant,
        decision="REJECT",
    )


# ── 最小流程 ────────────────────────────────────────────


def linear_dsl() -> dict:
    return {
        "workflow_key": "test",
        "version": 1,
        "business_object": "quotation",
        "trigger": {"type": "form_submit"},
        "nodes": [
            {"id": "start", "type": "trigger"},
            {
                "id": "approval",
                "type": "human_approval",
                "participant": "internal",
                "resolver": {"type": "manager_of", "of": "initiator"},
                "on_reject": {"action": "end", "result": "rejected"},
            },
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [["start", "approval"], ["approval", "end"]],
    }


class Test基本走訪:
    async def test_核准後流程完成(self, env):
        reset_state()
        result = await run_workflow(env, make_input(linear_dsl()), [approve("approval")])

        assert result.status is InstanceStatus.COMPLETED
        assert result.path == ["start", "approval", "end"]

    async def test_結束後回報狀態給業務資料庫(self, env):
        # 收件匣與報表查的是 PostgreSQL 不是 Temporal。
        # 不回報的話單據會永遠顯示「進行中」。
        reset_state()
        await run_workflow(env, make_input(linear_dsl()), [approve("approval")])

        assert len(STATE["finished"]) == 1
        assert STATE["finished"][0]["status"] == "COMPLETED"

    async def test_退回時也會回報(self, env):
        reset_state()
        await run_workflow(env, make_input(linear_dsl()), [reject("approval")])

        assert STATE["finished"][0]["status"] == "REJECTED"

    async def test_退回時以_rejected_結束(self, env):
        reset_state()
        result = await run_workflow(env, make_input(linear_dsl()), [reject("approval")])

        assert result.status is InstanceStatus.REJECTED
        assert "end" not in result.path, "退回不該經過 end 節點"

    async def test_缺少_trigger_節點時明確失敗(self, env):
        reset_state()
        dsl = linear_dsl()
        dsl["nodes"] = [n for n in dsl["nodes"] if n["type"] != "trigger"]

        result = await run_workflow(env, make_input(dsl))
        assert result.status is InstanceStatus.FAILED
        assert "trigger" in result.error

    async def test_找不到參與者時明確失敗而非靜默卡住(self, env):
        # 靜默卡住最糟：流程看似在跑，實際永遠沒人能處理。
        # interpreter 攔下 ApplicationError 並回 FAILED，
        # 因此是「結果為失敗」而非「拋出例外」。
        reset_state(participants=[])

        result = await run_workflow(env, make_input(linear_dsl()))

        assert result.status is InstanceStatus.FAILED
        assert "參與者" in result.error


class Test條件分支:
    def dsl(self) -> dict:
        return {
            "workflow_key": "test",
            "version": 1,
            "business_object": "quotation",
            "trigger": {"type": "form_submit"},
            "nodes": [
                {"id": "start", "type": "trigger"},
                {
                    "id": "gate",
                    "type": "condition",
                    "expression": "quotation.discount_rate > 0.15",
                },
                {"id": "high", "type": "action", "action": "noop.high"},
                {"id": "low", "type": "action", "action": "noop.low"},
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [
                ["start", "gate"],
                ["gate", "high", {"when": True}],
                ["gate", "low", {"when": False}],
                ["high", "end"],
                ["low", "end"],
            ],
        }

    async def test_條件成立走_true_分支(self, env):
        reset_state()
        result = await run_workflow(
            env, make_input(self.dsl(), {"quotation": {"discount_rate": 0.2}})
        )

        assert "high" in result.path
        assert "low" not in result.path

    async def test_條件不成立走_false_分支(self, env):
        reset_state()
        result = await run_workflow(
            env, make_input(self.dsl(), {"quotation": {"discount_rate": 0.1}})
        )

        assert "low" in result.path
        assert "high" not in result.path

    async def test_欄位缺漏時條件為假(self, env):
        # 設計流程時欄位可能還沒建，不該讓流程整個掛掉
        reset_state()
        result = await run_workflow(env, make_input(self.dsl(), {}))

        assert result.status is InstanceStatus.COMPLETED
        assert "low" in result.path


class Test退回路徑:
    def dsl(self) -> dict:
        return {
            "workflow_key": "test",
            "version": 1,
            "business_object": "quotation",
            "trigger": {"type": "form_submit"},
            "nodes": [
                {"id": "start", "type": "trigger"},
                {
                    "id": "approval",
                    "type": "human_approval",
                    "participant": "internal",
                    "resolver": {"type": "manager_of", "of": "initiator"},
                    "on_reject": {"action": "goto", "node": "revise"},
                },
                {
                    "id": "revise",
                    "type": "human_task",
                    "participant": "internal",
                    "assignee": {"type": "initiator"},
                },
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [
                ["start", "approval"],
                ["approval", "end"],
                ["revise", "approval"],
            ],
        }

    async def test_退回後可再次送審並通過(self, env):
        # 這是報價單最典型的路徑：主管退回、業務改完再送、這次過了。
        # 同一個節點會被走過兩次，task_id 也會重複產生，
        # 因此 Signal 必須帶 node_id 才能區分是哪一輪。
        reset_state()

        task_queue = f"test-{uuid.uuid4()}"
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

            # 第一輪：退回
            await signal_at(handle, reject("approval"))
            # 修改節點完成
            await signal_at(handle, approve("revise"))
            # 第二輪：通過
            await signal_at(handle, approve("approval"))

            result = await handle.result()

        assert result.status is InstanceStatus.COMPLETED
        assert result.path == [
            "start",
            "approval",
            "revise",
            "approval",
            "end",
        ], f"實際路徑：{result.path}"

    async def test_退回目標不存在時明確失敗(self, env):
        reset_state()
        dsl = self.dsl()
        node = next(n for n in dsl["nodes"] if n["id"] == "approval")
        node["on_reject"] = {"action": "goto", "node": "not_exist"}

        result = await run_workflow(env, make_input(dsl), [reject("approval")])

        assert result.status is InstanceStatus.FAILED
        assert "not_exist" in result.error


class Test並簽:
    def dsl(self, join: dict) -> dict:
        return {
            "workflow_key": "test",
            "version": 1,
            "business_object": "quotation",
            "trigger": {"type": "form_submit"},
            "nodes": [
                {"id": "start", "type": "trigger"},
                {
                    "id": "approval",
                    "type": "human_approval",
                    "participant": "internal",
                    "resolver": {"type": "role", "value": "approver"},
                    "join": join,
                    "on_reject": {"action": "end", "result": "rejected"},
                },
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [["start", "approval"], ["approval", "end"]],
        }

    def three_people(self):
        return [
            {"id": "u1", "display": "甲", "kind": "internal"},
            {"id": "u2", "display": "乙", "kind": "internal"},
            {"id": "u3", "display": "丙", "kind": "internal"},
        ]

    async def test_all_需要全員核准(self, env):
        reset_state(self.three_people())
        dsl = self.dsl({"completion": "ALL", "result": "ALL_SUCCESS"})

        result = await run_workflow(
            env,
            make_input(dsl),
            [
                approve("approval", 0, "u1"),
                approve("approval", 1, "u2"),
                approve("approval", 2, "u3"),
            ],
        )
        assert result.status is InstanceStatus.COMPLETED

    async def test_any_reject_一人否決立即結束(self, env):
        # 三人會簽，第一人否決時不該讓另外兩人白跑
        reset_state(self.three_people())
        dsl = self.dsl({"completion": "ALL", "result": "ANY_REJECT"})

        result = await run_workflow(
            env, make_input(dsl), [reject("approval", 0, "u1")]
        )

        assert result.status is InstanceStatus.REJECTED

    async def test_否決後其餘待辦被取消(self, env):
        reset_state(self.three_people())
        dsl = self.dsl({"completion": "ALL", "result": "ANY_REJECT"})

        await run_workflow(env, make_input(dsl), [reject("approval", 0, "u1")])

        # 未回應的兩人待辦必須被取消，否則收件匣會留下點不動的項目
        assert set(STATE["cancelled"]) == {"task-approval-1", "task-approval-2"}

    async def test_any_一人核准即前進(self, env):
        reset_state(self.three_people())
        dsl = self.dsl({"completion": "ANY", "result": "ALL_SUCCESS"})

        result = await run_workflow(
            env, make_input(dsl), [approve("approval", 0, "u1")]
        )
        assert result.status is InstanceStatus.COMPLETED


class Test冪等:
    async def test_重複_signal_不重複計算(self, env):
        # 前端重試或網路重送都可能送出兩次同樣的決策
        reset_state(
            [
                {"id": "u1", "display": "甲", "kind": "internal"},
                {"id": "u2", "display": "乙", "kind": "internal"},
            ]
        )
        dsl = Test並簽().dsl({"completion": "ALL", "result": "ALL_SUCCESS"})

        result = await run_workflow(
            env,
            make_input(dsl),
            [
                approve("approval", 0, "u1"),
                approve("approval", 0, "u1"),  # 重複
                approve("approval", 1, "u2"),
            ],
        )
        assert result.status is InstanceStatus.COMPLETED

    async def test_非當前節點的決策被忽略(self, env):
        reset_state()
        task_queue = f"test-{uuid.uuid4()}"

        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[DslInterpreter],
            activities=ACTIVITIES,
        ):
            handle = await env.client.start_workflow(
                DslInterpreter.run,
                make_input(linear_dsl()),
                id=f"wf-{uuid.uuid4()}",
                task_queue=task_queue,
            )

            await wait_for_node(handle, "approval")

            # 直接送而非用 signal_at：這個節點根本不存在，等不到它就緒。
            # 流程不該前進，也不該崩潰。
            await handle.signal(
                DslInterpreter.task_completed,
                TaskCompleted(
                    task_id="task-other-0",
                    node_id="other_node",
                    participant_id="u1",
                    decision="APPROVE",
                ),
            )

            state = await handle.query(DslInterpreter.current_state)
            assert state.current_node == "approval", "不該因為外來決策而前進"
            assert state.results == [], "不該記錄不屬於此節點的決策"

            await handle.signal(DslInterpreter.task_completed, approve("approval"))
            result = await handle.result()

        assert result.status is InstanceStatus.COMPLETED


class Test系統動作:
    def dsl(self, on_failure: dict | None = None) -> dict:
        action = {
            "id": "create_order",
            "type": "action",
            "action": "odoo.create_sale_order",
            "retry": {"max_attempts": 1},
        }
        if on_failure:
            action["on_failure"] = on_failure

        return {
            "workflow_key": "test",
            "version": 1,
            "business_object": "quotation",
            "trigger": {"type": "form_submit"},
            "nodes": [
                {"id": "start", "type": "trigger"},
                action,
                {
                    "id": "revise",
                    "type": "human_task",
                    "participant": "internal",
                    "assignee": {"type": "initiator"},
                },
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [
                ["start", "create_order"],
                ["create_order", "end"],
                ["revise", "end"],
            ],
        }

    async def test_動作輸出合併回業務物件(self, env):
        reset_state()
        result = await run_workflow(env, make_input(self.dsl()))

        assert result.output["business_object"]["create_order_done"] is True

    async def test_冪等鍵穩定(self, env):
        reset_state()
        inp = make_input(self.dsl())
        await run_workflow(env, inp)

        key = STATE["actions"][0]["idempotency_key"]
        assert key == f"{inp.instance_id}:create_order"

    async def test_失敗且有_on_failure_時走退回路徑(self, env):
        reset_state()
        STATE["action_fails"].add("create_order")

        result = await run_workflow(
            env, make_input(self.dsl({"action": "goto", "node": "revise"})),
            [approve("revise")],
        )

        assert "revise" in result.path

    async def test_失敗且無_on_failure_時流程失敗(self, env):
        reset_state()
        STATE["action_fails"].add("create_order")

        result = await run_workflow(env, make_input(self.dsl()))

        assert result.status is InstanceStatus.FAILED
        assert "create_order" in result.error


class Test通知:
    async def test_通知失敗不中斷流程(self, env):
        # 單據已核准，卻因寄信失敗讓整張單卡住，業務上說不通
        reset_state()
        dsl = {
            "workflow_key": "test",
            "version": 1,
            "business_object": "quotation",
            "trigger": {"type": "form_submit"},
            "nodes": [
                {"id": "start", "type": "trigger"},
                {
                    "id": "notify",
                    "type": "notification",
                    "channel": ["email"],
                    "to": ["initiator"],
                },
                {"id": "end", "type": "end", "result": "completed"},
            ],
            "edges": [["start", "notify"], ["notify", "end"]],
        }

        result = await run_workflow(env, make_input(dsl))
        assert result.status is InstanceStatus.COMPLETED


class Test報價單_fixture:
    """用真實的 DSL fixture 跑完整路徑"""

    def dsl(self) -> dict:
        return json.loads((FIXTURES / "quotation_approval_v1.json").read_text())

    async def test_低折扣路徑跳過財務簽核(self, env):
        reset_state()
        result = await run_workflow(
            env,
            make_input(self.dsl(), {"quotation": {"discount_rate": 0.1}}),
            [
                approve("manager_approval"),
                approve("customer_review"),
            ],
        )

        assert result.status is InstanceStatus.COMPLETED
        assert "finance_approval" not in result.path
        assert result.path == [
            "start",
            "manager_approval",
            "discount_gate",
            "publish_to_customer",
            "customer_review",
            "create_order",
            "notify_done",
            "end",
        ], f"實際路徑：{result.path}"

    async def test_高折扣路徑加簽財務(self, env):
        reset_state()
        result = await run_workflow(
            env,
            make_input(self.dsl(), {"quotation": {"discount_rate": 0.2}}),
            [
                approve("manager_approval"),
                approve("finance_approval"),
                approve("customer_review"),
            ],
        )

        assert result.status is InstanceStatus.COMPLETED
        assert "finance_approval" in result.path

    async def test_主管退回走修改再送審(self, env):
        reset_state()
        task_queue = f"test-{uuid.uuid4()}"

        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[DslInterpreter],
            activities=ACTIVITIES,
        ):
            handle = await env.client.start_workflow(
                DslInterpreter.run,
                make_input(self.dsl(), {"quotation": {"discount_rate": 0.1}}),
                id=f"wf-{uuid.uuid4()}",
                task_queue=task_queue,
            )

            await signal_at(handle, reject("manager_approval"))
            await signal_at(handle, approve("revise"))
            await signal_at(handle, approve("manager_approval"))
            await signal_at(handle, approve("customer_review"))

            result = await handle.result()

        assert result.status is InstanceStatus.COMPLETED
        assert result.path.count("manager_approval") == 2
        assert "revise" in result.path
