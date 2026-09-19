"""
時間快轉測試（模擬簽核用）

含 `timeout: {after: P2D}` 的流程在模擬時原本要等兩天才看得到
逾時行為，等於無法驗證。快轉讓等待立刻視為到期。

**設計上的關鍵：快轉不假造決策。**
它跑的是真正的逾時處理，與真的等到逾時走同一條路。
假造一個 APPROVE 會讓模擬顯示「通過了」，而正式環境設的
AUTO_REJECT 其實是退回——那比不給快轉更糟，因為使用者
會帶著錯誤的結論上線。

**只限沙箱。** 快轉會讓單據在沒人簽的情況下前進，
正式環境開放它等同開放繞過簽核。
"""

from __future__ import annotations

import asyncio
import uuid

import pytest
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker

from dsl.models import InstanceInput, InstanceStatus
from workflows.interpreter import DslInterpreter

from tests.test_interpreter import STATE, reset_state, wait_for_node
from tests.test_timeout import ACTIVITIES

pytestmark = pytest.mark.asyncio


@pytest.fixture(scope="module")
async def env():
    # 用本機環境而非 time-skipping：快轉測的是 Signal，
    # 時鐘自己會跑掉的環境反而分不清是誰讓流程前進的。
    async with await WorkflowEnvironment.start_local() as e:
        yield e


def sandbox_input(dsl: dict, is_sandbox: bool = True) -> InstanceInput:
    return InstanceInput(
        tenant_id="11111111-1111-1111-1111-111111111111",
        instance_id=str(uuid.uuid4()),
        workflow_version_id=str(uuid.uuid4()),
        dsl=dsl,
        business_object={},
        is_sandbox=is_sandbox,
    )


def dsl_with(policy: str) -> dict:
    """一個等兩天的簽核節點"""
    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {
                "id": "approve",
                "type": "human_approval",
                "label": "主管簽核",
                "timeout": {"after": "P2D", "policy": policy},
                "on_reject": {"action": "end", "result": "rejected"},
            },
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [["start", "approve"], ["approve", "end"]],
    }


async def still_waiting(handle) -> bool:
    """流程是否還停在簽核節點

    current_state 的回傳在不同環境下是 dataclass 或 dict，
    兩種都接受——測的是流程位置，不是序列化形式。
    """
    state = await handle.query("current_state")
    nodes = (
        state.get("active_nodes") or [state.get("current_node")]
        if isinstance(state, dict)
        else state.active_nodes
    )
    return nodes == ["approve"]


async def start(env, inp: InstanceInput):
    queue = f"skip-{uuid.uuid4()}"
    worker = Worker(
        env.client,
        task_queue=queue,
        workflows=[DslInterpreter],
        activities=ACTIVITIES,
    )
    handle = await env.client.start_workflow(
        DslInterpreter.run, inp, id=f"wf-{uuid.uuid4()}", task_queue=queue
    )
    return worker, handle


class Test快轉走真正的逾時處理:
    async def test_AUTO_APPROVE_快轉後視為通過(self, env):
        # 不是假造 APPROVE，是讓 P2D 立刻到期後由策略決定
        reset_state()
        worker, handle = await start(env, sandbox_input(dsl_with("AUTO_APPROVE")))
        async with worker:
            await wait_for_node(handle, "approve")
            await handle.signal("skip_time", "approve")
            result = await handle.result()

        assert result.status is InstanceStatus.COMPLETED

    async def test_AUTO_REJECT_快轉後是退回而非通過(self, env):
        # **這條是整份測試的重點。**
        # 若快轉是「假造一個 APPROVE」，這裡會是 COMPLETED，
        # 使用者就會以為逾時會放行，而正式環境其實是退回。
        reset_state()
        worker, handle = await start(env, sandbox_input(dsl_with("AUTO_REJECT")))
        async with worker:
            await wait_for_node(handle, "approve")
            await handle.signal("skip_time", "approve")
            result = await handle.result()

        assert result.status is InstanceStatus.REJECTED, (
            "AUTO_REJECT 逾時必須是退回。快轉若假造決策，這裡會變成通過"
        )


class Test授權:
    async def test_正式租戶的實例拒絕快轉(self, env):
        # **安全邊界。** 快轉會讓單據在沒人簽的情況下前進。
        # HTTP 端已擋一層，這裡再擋一層：Signal 是能繞過 HTTP
        # 的介面，而 is_sandbox 預設 False 代表既有的執行中實例
        # 也一併受保護。
        reset_state()
        worker, handle = await start(
            env, sandbox_input(dsl_with("AUTO_APPROVE"), is_sandbox=False)
        )
        async with worker:
            await wait_for_node(handle, "approve")
            await handle.signal("skip_time", "approve")

            # 快轉被忽略，流程仍停在簽核節點
            await asyncio.sleep(1)
            assert await still_waiting(handle), (
                "正式租戶的實例不可被快轉，流程必須還停在原節點"
            )

            await handle.cancel()
            try:
                await handle.result()
            except Exception:  # noqa: BLE001
                pass

    async def test_非活躍節點的快轉被忽略(self, env):
        # 送錯節點不可讓別的節點前進——平行分支時尤其危險
        reset_state()
        worker, handle = await start(env, sandbox_input(dsl_with("AUTO_APPROVE")))
        async with worker:
            await wait_for_node(handle, "approve")
            await handle.signal("skip_time", "not_a_real_node")

            await asyncio.sleep(1)
            assert await still_waiting(handle)

            await handle.signal("skip_time", "approve")
            result = await handle.result()
        assert result.status is InstanceStatus.COMPLETED
