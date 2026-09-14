"""
Temporal Workflow PoC 測試

驗證任務 0.2 的三件事：
  1. 並簽 ALL_SUCCESS，一人 REJECT 立即結束並取消其他人
  2. 簽核人依金額動態產生
  3. Worker 重啟後狀態不丟

使用 Temporal 內建測試環境，不需 Docker。
"""

from __future__ import annotations

import uuid
from datetime import timedelta

import activities
import pytest
import pytest_asyncio
from models import (
    ApprovalInput,
    ApprovalOutput,
    Completion,
    Decision,
    JoinPolicy,
    ResultPolicy,
    TaskCompleted,
    Verdict,
)
from temporalio import activity
from temporalio.client import Client, WorkflowFailureError
from temporalio.exceptions import ApplicationError
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import Worker
from workflow import ApprovalWorkflow

TASK_QUEUE = "poc-approval"


@pytest_asyncio.fixture
async def env():
    """
    time-skipping 環境：逾期測試不必真的等七天。

    代價：時鐘會在 Workflow 等待時自動快轉，因此需要「真實等待」的測試
    （例如輪詢查詢、跨 Worker 重啟）不能用這個環境，改用 env_real。
    """
    async with await WorkflowEnvironment.start_time_skipping() as e:
        yield e


@pytest_asyncio.fixture
async def env_real():
    """
    真實 Temporal 伺服器（docker compose up 起的）。

    用於需要穩定時鐘的測試：跨 Worker 重啟、失敗路徑。
    time-skipping 環境的虛擬時鐘會干擾這類測試。

    需先執行：
        cd poc/temporal && docker compose up -d

    伺服器未就緒時該測試會 skip，不算失敗。
    """
    try:
        client = await Client.connect("localhost:7233", namespace="default")
    except Exception as e:
        pytest.skip(f"真實 Temporal 未就緒，跳過：{e}")
    yield _RealEnv(client)


class _RealEnv:
    """讓真實 client 符合 WorkflowEnvironment 的介面（只需 .client）。"""

    def __init__(self, client: Client) -> None:
        self.client = client


@pytest_asyncio.fixture
async def worker(env: WorkflowEnvironment):
    activities.reset()
    async with Worker(
        env.client,
        task_queue=TASK_QUEUE,
        workflows=[ApprovalWorkflow],
        activities=activities.ALL_ACTIVITIES,
    ):
        yield env.client


def new_input(total: int, **kw) -> ApprovalInput:
    return ApprovalInput(
        instance_id=f"QT-{uuid.uuid4().hex[:8]}",
        business_object={"total": total},
        **kw,
    )


async def start(client: Client, inp: ApprovalInput):
    return await client.start_workflow(
        ApprovalWorkflow.run,
        inp,
        id=f"wf-{inp.instance_id}",
        task_queue=TASK_QUEUE,
        result_type=ApprovalOutput,   # 否則 result() 回 dict 而非 dataclass
    )


async def wait_for_tasks(handle, expected: int) -> list[str]:
    """
    等 Activity 建好 task 後取得 task_id。

    用 query 輪詢並在每輪之間讓出事件迴圈。query 是真實 gRPC 往返，
    不會推進 time-skipping 的虛擬時鐘（只有 Workflow 等待計時器才會）。
    """
    import asyncio

    last = None
    for _ in range(100):
        try:
            state = await handle.query(ApprovalWorkflow.current_state)
        except Exception:
            await asyncio.sleep(0)
            continue
        last = state
        if state["total"] == expected and state["pending_task_ids"]:
            return state["pending_task_ids"]
        await asyncio.sleep(0)  # 讓 Worker 有機會執行 Activity
    raise AssertionError(f"等不到 {expected} 個 task，最後狀態：{last}")


def expected_task_ids(instance_id: str, participant_ids: list[str]) -> list[str]:
    """
    task_id 由 create_human_tasks 以 {instance_id}:{participant_id} 產生。
    測試可直接推算，不必查詢，避免輪詢干擾時鐘。
    """
    return [f"{instance_id}:{p}" for p in participant_ids]


# ── 驗證 1：並簽與 REJECT 立即中斷 ──────────────────────────────


@pytest.mark.asyncio
async def test_three_way_approval_all_success(worker: Client):
    """3 人並簽全部同意 → CONTINUE，無人被取消"""
    inp = new_input(500_000, join=JoinPolicy(completion=Completion.ALL, result=ResultPolicy.ALL_SUCCESS))
    handle = await start(worker, inp)
    tasks = await wait_for_tasks(handle, 2)  # 50 萬只有主管與財務

    for t in tasks:
        await handle.signal(
            ApprovalWorkflow.task_completed,
            TaskCompleted(task_id=t, participant_id=t.split(":")[-1], decision=Decision.APPROVE),
        )

    out = await handle.result()
    assert out.verdict is Verdict.CONTINUE
    assert len(out.results) == 2
    assert out.cancelled_task_ids == []


@pytest.mark.asyncio
async def test_one_reject_cancels_others_immediately(worker: Client):
    """關鍵驗證：3 人並簽，第 1 人拒絕即結束，另外 2 人的 task 被取消"""
    inp = new_input(
        2_000_000,  # 超過 100 萬，3 人
        join=JoinPolicy(completion=Completion.ALL, result=ResultPolicy.ANY_REJECT),
    )
    handle = await start(worker, inp)
    tasks = await wait_for_tasks(handle, 3)

    # 只有第一個人回應，且是拒絕
    await handle.signal(
        ApprovalWorkflow.task_completed,
        TaskCompleted(task_id=tasks[0], participant_id="u-mgr", decision=Decision.REJECT, comment="價格過低"),
    )

    out = await handle.result()
    assert out.verdict is Verdict.REJECT
    assert len(out.results) == 1, "不應等待其他人"
    assert set(out.cancelled_task_ids) == set(tasks[1:]), "其餘 task 應被取消"

    # 驗證 Activity 真的把狀態改掉
    for t in tasks[1:]:
        assert activities.TASK_STORE[t]["status"] == "CANCELLED"
    assert ("cancel_human_tasks", tasks[1:]) in activities.CALL_LOG


# ── 驗證 2：動態簽核人 ────────────────────────────────────────


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "total,expected_count,expect_cfo",
    [
        (900_000, 2, False),
        (1_000_000, 2, False),   # 邊界：等於 100 萬不加簽
        (1_000_001, 3, True),    # 邊界：超過即加簽
        (5_000_000, 3, True),
    ],
)
async def test_dynamic_approvers_by_amount(worker: Client, total, expected_count, expect_cfo):
    """金額門檻決定是否加簽 CFO"""
    handle = await start(worker, new_input(total))
    tasks = await wait_for_tasks(handle, expected_count)

    state = await handle.query(ApprovalWorkflow.current_state)
    assert state["total"] == expected_count
    assert ("陳財務長" in state["participants"]) is expect_cfo

    for t in tasks:
        await handle.signal(
            ApprovalWorkflow.task_completed,
            TaskCompleted(task_id=t, participant_id=t.split(":")[-1], decision=Decision.APPROVE),
        )
    out = await handle.result()
    assert out.verdict is Verdict.CONTINUE
    assert len(out.participants) == expected_count


@activity.defn(name="resolve_approvers")
async def _resolve_nobody(_business_object: dict) -> list:
    """模擬簽核人解析為空，例如部門沒設主管。"""
    return []


@pytest.mark.asyncio
async def test_empty_resolution_fails_explicitly(env_real: WorkflowEnvironment):
    """
    簽核人解析為空時，流程明確失敗而非靜默卡住。

    這在真實情境很常見：部門沒設主管、角色沒指派人員、客戶沒填聯絡人。
    正式版應進 FAILED_RESOLUTION 狀態並通知 admin 指定人員後續跑。
    """
    activities.reset()
    queue = f"poc-empty-{uuid.uuid4().hex[:8]}"

    async with Worker(
        env_real.client,
        task_queue=queue,
        workflows=[ApprovalWorkflow],
        activities=[_resolve_nobody, activities.create_human_tasks, activities.cancel_human_tasks],
    ):
        handle = await env_real.client.start_workflow(
            ApprovalWorkflow.run,
            new_input(500_000),
            id=f"wf-empty-{uuid.uuid4().hex[:8]}",
            task_queue=queue,
            result_type=ApprovalOutput,
            # non_retryable 的 ApplicationError 會立刻讓流程失敗，
            # 但 execution timeout 可避免測試在意外情況下永久掛住
            execution_timeout=timedelta(seconds=30),
        )
        with pytest.raises(WorkflowFailureError) as ei:
            await handle.result()

    # 錯誤型別與訊息在 __cause__，不在頂層的 WorkflowFailureError
    cause = ei.value.__cause__
    assert isinstance(cause, ApplicationError), f"預期 ApplicationError，實際 {type(cause)}"
    assert cause.type == "RESOLVE_EMPTY"
    assert cause.non_retryable, "解析為空是設定問題，重試無用，必須標為不可重試"


# ── 驗證 3：逾期與持久性 ──────────────────────────────────────


@pytest.mark.asyncio
async def test_timeout_when_nobody_responds(worker: Client):
    """逾期且無人回應 → TIMEOUT。time-skipping 讓 7 天瞬間過完。"""
    inp = new_input(
        500_000,
        join=JoinPolicy(completion=Completion.ALL),
        timeout_seconds=int(timedelta(days=7).total_seconds()),
    )
    handle = await start(worker, inp)
    await wait_for_tasks(handle, 2)

    out = await handle.result()
    assert out.verdict is Verdict.TIMEOUT
    assert len(out.cancelled_task_ids) == 2


@pytest.mark.asyncio
async def test_long_wait_does_not_block(worker: Client):
    """等待 30 天期間流程不佔用資源，時間跳過後仍可正常簽核。"""
    inp = new_input(
        500_000,
        timeout_seconds=int(timedelta(days=30).total_seconds()),
    )
    handle = await start(worker, inp)
    tasks = await wait_for_tasks(handle, 2)

    # 模擬第 29 天才有人簽
    for t in tasks:
        await handle.signal(
            ApprovalWorkflow.task_completed,
            TaskCompleted(task_id=t, participant_id=t.split(":")[-1], decision=Decision.APPROVE),
        )
    out = await handle.result()
    assert out.verdict is Verdict.CONTINUE


@pytest.mark.asyncio
async def test_state_survives_worker_restart(env_real: WorkflowEnvironment):
    """
    關鍵驗證：Worker 在等待中被關閉再重啟，流程狀態不丟。

    這是 Temporal 最核心的價值，也是自研 Runtime 最難做對的部分。

    用真實時鐘環境（env_real）。time-skipping 會在 Worker 關閉期間快轉時間，
    導致查詢逾時，那是測試環境特性而非產品行為。
    """
    env = env_real
    activities.reset()
    inp = new_input(2_000_000)   # 三人：主管、財務、CFO
    tasks = expected_task_ids(inp.instance_id, ["u-mgr", "u-fin", "u-cfo"])

    # 第一個 Worker：啟動流程，讓一個人先簽
    async with Worker(
        env.client, task_queue=TASK_QUEUE,
        workflows=[ApprovalWorkflow], activities=activities.ALL_ACTIVITIES,
    ):
        handle = await start(env.client, inp)
        await wait_for_tasks(handle, 3)
        await handle.signal(
            ApprovalWorkflow.task_completed,
            TaskCompleted(task_id=tasks[0], participant_id="u-mgr", decision=Decision.APPROVE),
        )
        state = await handle.query(ApprovalWorkflow.current_state)
        assert state["completed"] == 1

    # Worker 已關閉。流程仍在，狀態留在 Temporal。

    # 第二個 Worker：接手，剩下的人簽完
    async with Worker(
        env.client, task_queue=TASK_QUEUE,
        workflows=[ApprovalWorkflow], activities=activities.ALL_ACTIVITIES,
    ):
        state = await handle.query(ApprovalWorkflow.current_state)
        assert state["completed"] == 1, "重啟後第一人的簽核結果應仍在"

        for t in tasks[1:]:
            await handle.signal(
                ApprovalWorkflow.task_completed,
                TaskCompleted(task_id=t, participant_id=t.split(":")[-1], decision=Decision.APPROVE),
            )
        out = await handle.result()

    assert out.verdict is Verdict.CONTINUE
    assert len(out.results) == 3


@pytest.mark.asyncio
async def test_duplicate_signal_is_idempotent(worker: Client):
    """同一 task 重複送 Signal 不應重複計算。前端重試或網路重送都可能發生。"""
    handle = await start(worker, new_input(500_000))
    tasks = await wait_for_tasks(handle, 2)

    sig = TaskCompleted(task_id=tasks[0], participant_id="u-mgr", decision=Decision.APPROVE)
    await handle.signal(ApprovalWorkflow.task_completed, sig)
    await handle.signal(ApprovalWorkflow.task_completed, sig)
    await handle.signal(ApprovalWorkflow.task_completed, sig)

    state = await handle.query(ApprovalWorkflow.current_state)
    assert state["completed"] == 1, "重複 Signal 應被忽略"

    await handle.signal(
        ApprovalWorkflow.task_completed,
        TaskCompleted(task_id=tasks[1], participant_id="u-fin", decision=Decision.APPROVE),
    )
    out = await handle.result()
    assert len(out.results) == 2
