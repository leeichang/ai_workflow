"""
並行分支與匯合測試

Parallel + Join 是企業流程的核心能力：多人會簽、跨部門協作。
沒有它，「部門主管、財務、採購同時簽核，全部簽完才繼續」做不到。

設計決定（見 docs/需求規劃/202609/04）：
  - 用 Temporal 原生並行（asyncio.gather），不建 parallel_group 資料表。
    Temporal 本身就持久化每個分支的進度，再存一份 SQL 會有兩份狀態要對帳。
  - 分支可含多個節點，是完整的子流程而非單一簽核。

與單節點多參與者的區別：
  單節點多參與者  一個 human_approval，三個人簽 → 用 node.join
  並行分支        三個不同的節點各自進行  → 用 parallel/join 節點
兩者都用同一個 policy engine 判定，語意一致。
"""

from __future__ import annotations

import asyncio
import uuid

import pytest
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker

from dsl.models import InstanceInput, InstanceStatus, TaskCompleted
from workflows.interpreter import DslInterpreter

from tests.test_interpreter import (
    ACTIVITIES,
    STATE,
    make_input,
    reset_state,
)

pytestmark = pytest.mark.asyncio


@pytest.fixture(scope="module")
async def env():
    async with await WorkflowEnvironment.start_local() as e:
        yield e


# ── 並行專用的工具 ──────────────────────────────────────
#
# 不能沿用 test_interpreter 的 wait_for_node：那個函式假設
# 只有一個 current node，並行時同時有多個節點在等待。


async def wait_for_task(handle, node_id: str, timeout: float = 15.0) -> None:
    """等指定節點出現待辦

    並行時 current_node 不再是單一值，改查 active_nodes。
    """
    deadline = asyncio.get_event_loop().time() + timeout
    while asyncio.get_event_loop().time() < deadline:
        state = await handle.query(DslInterpreter.current_state)
        if node_id in (state.active_nodes or []):
            return
        await asyncio.sleep(0.05)

    state = await handle.query(DslInterpreter.current_state)
    raise AssertionError(
        f"等不到節點 {node_id} 的待辦。目前活躍：{state.active_nodes}"
    )


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


async def start(env: WorkflowEnvironment, inp: InstanceInput):
    """啟動流程並回傳 handle 與 worker context

    並行測試不能像單線流程那樣「依序送 signal」——
    分支是同時活躍的，要能任意順序送。
    """
    task_queue = f"par-{uuid.uuid4()}"
    worker = Worker(
        env.client,
        task_queue=task_queue,
        workflows=[DslInterpreter],
        activities=ACTIVITIES,
    )
    await worker.__aenter__()
    handle = await env.client.start_workflow(
        DslInterpreter.run,
        inp,
        id=f"wf-{uuid.uuid4()}",
        task_queue=task_queue,
    )
    return handle, worker


async def signal_all(handle, sigs: list[TaskCompleted]) -> None:
    """等各節點就緒後送出決策，順序依傳入順序"""
    for sig in sigs:
        await wait_for_task(handle, sig.node_id)
        await handle.signal(DslInterpreter.task_completed, sig)


# ── DSL 建構 ────────────────────────────────────────────


def three_way_dsl(join_policy: dict | None = None) -> dict:
    """三方並簽：財務、法務、採購同時進行"""
    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {"id": "split", "type": "parallel", "label": "三方並簽"},
            {
                "id": "finance",
                "type": "human_approval",
                "label": "財務簽核",
                "resolver": {"type": "role", "value": "finance_manager"},
            },
            {
                "id": "legal",
                "type": "human_approval",
                "label": "法務簽核",
                "resolver": {"type": "role", "value": "legal_manager"},
            },
            {
                "id": "purchase",
                "type": "human_approval",
                "label": "採購簽核",
                "resolver": {"type": "role", "value": "purchase_manager"},
            },
            {
                "id": "merge",
                "type": "join",
                "label": "並簽完成",
                "join": join_policy or {"completion": "ALL", "result": "ALL_SUCCESS"},
            },
            {"id": "gm", "type": "human_approval", "label": "總經理簽核"},
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [
            ["start", "split"],
            ["split", "finance"],
            ["split", "legal"],
            ["split", "purchase"],
            ["finance", "merge"],
            ["legal", "merge"],
            ["purchase", "merge"],
            ["merge", "gm"],
            ["gm", "end"],
        ],
    }


def multi_node_branch_dsl() -> dict:
    """分支含多個節點

    財務分支：簽核 → 通知
    法務分支：簽核
    """
    return {
        "nodes": [
            {"id": "start", "type": "trigger"},
            {"id": "split", "type": "parallel"},
            {"id": "finance", "type": "human_approval", "label": "財務簽核"},
            {
                "id": "notify_ap",
                "type": "notification",
                "channel": ["email"],
                "to": ["ap@demo.local"],
            },
            {"id": "legal", "type": "human_approval", "label": "法務簽核"},
            {
                "id": "merge",
                "type": "join",
                "join": {"completion": "ALL", "result": "ALL_SUCCESS"},
            },
            {"id": "end", "type": "end", "result": "completed"},
        ],
        "edges": [
            ["start", "split"],
            ["split", "finance"],
            ["finance", "notify_ap"],
            ["notify_ap", "merge"],
            ["split", "legal"],
            ["legal", "merge"],
            ["merge", "end"],
        ],
    }


# ── 測試 ────────────────────────────────────────────────


class Test基本並行:
    async def test_三個分支同時產生待辦(self, env):
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            for node in ("finance", "legal", "purchase"):
                await wait_for_task(handle, node)

            state = await handle.query(DslInterpreter.current_state)
            # 三個節點同時活躍，不是依序
            assert set(state.active_nodes) == {"finance", "legal", "purchase"}
        finally:
            await handle.cancel()
            await worker.__aexit__(None, None, None)

    async def test_全部核准後流程繼續(self, env):
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            await signal_all(
                handle,
                [approve("finance"), approve("legal"), approve("purchase")],
            )
            # 並簽完成後走到總經理
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))

            result = await handle.result()
            assert result.status is InstanceStatus.COMPLETED
        finally:
            await worker.__aexit__(None, None, None)

    async def test_路徑包含所有分支節點(self, env):
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            await signal_all(
                handle,
                [approve("finance"), approve("legal"), approve("purchase")],
            )
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))
            result = await handle.result()

            # 三個分支都要出現在路徑裡，供稽核
            for node in ("split", "finance", "legal", "purchase", "merge", "gm"):
                assert node in result.path, f"{node} 不在路徑 {result.path}"
        finally:
            await worker.__aexit__(None, None, None)

    async def test_簽核順序不影響結果(self, env):
        """並行的意義就是誰先誰後都可以"""
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            # 反過來簽
            await signal_all(
                handle,
                [approve("purchase"), approve("legal"), approve("finance")],
            )
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))

            result = await handle.result()
            assert result.status is InstanceStatus.COMPLETED
        finally:
            await worker.__aexit__(None, None, None)

    async def test_未全部簽完時不前進(self, env):
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            await signal_all(handle, [approve("finance"), approve("legal")])
            await asyncio.sleep(0.5)

            state = await handle.query(DslInterpreter.current_state)
            # 採購還沒簽，不該走到 gm
            assert "gm" not in (state.active_nodes or [])
            assert "purchase" in (state.active_nodes or [])
        finally:
            await handle.cancel()
            await worker.__aexit__(None, None, None)


class Test分支含多節點:
    async def test_分支內的節點依序執行(self, env):
        reset_state()
        handle, worker = await start(env, make_input(multi_node_branch_dsl()))
        try:
            await signal_all(handle, [approve("finance"), approve("legal")])
            result = await handle.result()

            assert result.status is InstanceStatus.COMPLETED
            # 財務分支的通知節點要執行到
            assert any(
                n["node_id"] == "notify_ap" for n in STATE["notifications"]
            ), "分支內的通知節點沒有執行"
        finally:
            await worker.__aexit__(None, None, None)

    async def test_較長的分支不會被較短的搶先結束(self, env):
        """法務只有一關，財務有兩關。join 要等兩邊都到齊"""
        reset_state()
        handle, worker = await start(env, make_input(multi_node_branch_dsl()))
        try:
            # 先簽法務（短分支），此時財務還沒動
            await signal_all(handle, [approve("legal")])
            await asyncio.sleep(0.5)

            state = await handle.query(DslInterpreter.current_state)
            assert "finance" in (state.active_nodes or []), "財務分支應該還在等"

            await signal_all(handle, [approve("finance")])
            result = await handle.result()
            assert result.status is InstanceStatus.COMPLETED
        finally:
            await worker.__aexit__(None, None, None)


class TestJoin完成策略:
    async def test_ANY_任一完成即前進(self, env):
        reset_state()
        dsl = three_way_dsl({"completion": "ANY", "result": "ALL_SUCCESS"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(handle, [approve("finance")])

            # 只簽一個就該走到 gm
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))

            result = await handle.result()
            assert result.status is InstanceStatus.COMPLETED
        finally:
            await worker.__aexit__(None, None, None)

    async def test_ANY_未完成的分支待辦被取消(self, env):
        """否則使用者的收件匣會留下永遠點不動的項目"""
        reset_state()
        dsl = three_way_dsl({"completion": "ANY", "result": "ALL_SUCCESS"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(handle, [approve("finance")])
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))
            await handle.result()

            cancelled = set(STATE["cancelled"])
            assert "task-legal-0" in cancelled
            assert "task-purchase-0" in cancelled
        finally:
            await worker.__aexit__(None, None, None)

    async def test_N_OF_M_達到門檻即前進(self, env):
        reset_state()
        dsl = three_way_dsl({"completion": "N_OF_M", "n": 2, "result": "ALL_SUCCESS"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(handle, [approve("finance"), approve("legal")])

            # 三取二，簽兩個就夠
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))

            result = await handle.result()
            assert result.status is InstanceStatus.COMPLETED
        finally:
            await worker.__aexit__(None, None, None)

    async def test_N_OF_M_未達門檻繼續等(self, env):
        reset_state()
        dsl = three_way_dsl({"completion": "N_OF_M", "n": 3, "result": "ALL_SUCCESS"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(handle, [approve("finance"), approve("legal")])
            await asyncio.sleep(0.5)

            state = await handle.query(DslInterpreter.current_state)
            assert "gm" not in (state.active_nodes or [])
        finally:
            await handle.cancel()
            await worker.__aexit__(None, None, None)


class TestJoin結果策略:
    async def test_ALL_SUCCESS_任一退回則流程退回(self, env):
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            await signal_all(
                handle,
                [approve("finance"), reject("legal"), approve("purchase")],
            )
            result = await handle.result()

            assert result.status is InstanceStatus.REJECTED
        finally:
            await worker.__aexit__(None, None, None)

    async def test_ANY_REJECT_第一個退回就不等其他人(self, env):
        """三人會簽，第一人否決時不該讓另外兩人白跑"""
        reset_state()
        dsl = three_way_dsl({"completion": "ALL", "result": "ANY_REJECT"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(handle, [reject("finance")])
            result = await handle.result()

            assert result.status is InstanceStatus.REJECTED
            # 另外兩人的待辦要取消
            cancelled = set(STATE["cancelled"])
            assert "task-legal-0" in cancelled
            assert "task-purchase-0" in cancelled
        finally:
            await worker.__aexit__(None, None, None)

    async def test_MAJORITY_過半通過(self, env):
        reset_state()
        dsl = three_way_dsl({"completion": "ALL", "result": "MAJORITY"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(
                handle,
                [approve("finance"), approve("legal"), reject("purchase")],
            )
            await wait_for_task(handle, "gm")
            await handle.signal(DslInterpreter.task_completed, approve("gm"))

            result = await handle.result()
            # 2 比 1，過半
            assert result.status is InstanceStatus.COMPLETED
        finally:
            await worker.__aexit__(None, None, None)

    async def test_MAJORITY_未過半則退回(self, env):
        reset_state()
        dsl = three_way_dsl({"completion": "ALL", "result": "MAJORITY"})
        handle, worker = await start(env, make_input(dsl))
        try:
            await signal_all(
                handle,
                [approve("finance"), reject("legal"), reject("purchase")],
            )
            result = await handle.result()

            assert result.status is InstanceStatus.REJECTED
        finally:
            await worker.__aexit__(None, None, None)


class Test決策歸屬:
    async def test_送給某分支的決策不影響其他分支(self, env):
        """並行時多個節點同時等待，決策必須對應到正確的節點"""
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            await signal_all(handle, [approve("finance")])
            await asyncio.sleep(0.3)

            state = await handle.query(DslInterpreter.current_state)
            # 財務簽完了，法務與採購還在
            active = set(state.active_nodes or [])
            assert "legal" in active
            assert "purchase" in active
            assert "finance" not in active
        finally:
            await handle.cancel()
            await worker.__aexit__(None, None, None)

    async def test_不存在的節點決策被忽略(self, env):
        reset_state()
        handle, worker = await start(env, make_input(three_way_dsl()))
        try:
            await wait_for_task(handle, "finance")
            # 送一個不屬於任何活躍節點的決策
            await handle.signal(
                DslInterpreter.task_completed,
                TaskCompleted(
                    task_id="task-ghost-0",
                    node_id="ghost",
                    participant_id="u1",
                    decision="APPROVE",
                ),
            )
            await asyncio.sleep(0.3)

            state = await handle.query(DslInterpreter.current_state)
            # 三個分支都還在，沒有被影響
            assert set(state.active_nodes or []) == {"finance", "legal", "purchase"}
        finally:
            await handle.cancel()
            await worker.__aexit__(None, None, None)


class Test錯誤處理:
    async def test_parallel_沒有分支時明確失敗(self, env):
        reset_state()
        dsl = {
            "nodes": [
                {"id": "start", "type": "trigger"},
                {"id": "split", "type": "parallel"},
                {"id": "end", "type": "end", "result": "completed"},
            ],
            # split 沒有任何出邊
            "edges": [["start", "split"]],
        }
        handle, worker = await start(env, make_input(dsl))
        try:
            result = await handle.result()
            assert result.status is InstanceStatus.FAILED
            assert "分支" in (result.error or "")
        finally:
            await worker.__aexit__(None, None, None)

    async def test_分支沒有通往_join_時明確失敗(self, env):
        """在建立待辦之前就該擋下來

        等分支跑完才發現沒有匯合點，使用者已經簽完名了才被告知
        流程設計有問題——那些簽核白做了。
        """
        reset_state()
        dsl = {
            "nodes": [
                {"id": "start", "type": "trigger"},
                {"id": "split", "type": "parallel"},
                {"id": "a", "type": "human_approval"},
                {"id": "end", "type": "end", "result": "completed"},
            ],
            # a 走到 end 而非 join
            "edges": [["start", "split"], ["split", "a"], ["a", "end"]],
        }
        handle, worker = await start(env, make_input(dsl))
        try:
            result = await handle.result()
            assert result.status is InstanceStatus.FAILED
            assert "join" in (result.error or "").lower() or "匯合" in (
                result.error or ""
            )
            # 不該有任何待辦被建立
            assert STATE["created"] == []
        finally:
            await worker.__aexit__(None, None, None)
