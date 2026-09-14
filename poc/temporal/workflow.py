"""
並簽 Workflow PoC

驗證三件事（任務 0.2）：
  1. 3 人並簽 ALL_SUCCESS，其中 1 人 REJECT 立即結束並取消其他 2 人
  2. 簽核人由「金額 > 100 萬加 CFO」動態產生
  3. Workflow 在等待中重啟 Worker，狀態不丟；發布新版程式碼後舊實例以 patch 續跑

設計約束（正式版必須維持）：
  - Workflow 內不得有 IO、不得讀時鐘、不得用亂數
  - 簽核人解析在 Activity 內（呼叫 Rust API），不在 Workflow 內
  - Policy Engine 是純函數，可在 Workflow 內直接呼叫
  - Human Task 的建立與取消都是 Activity
"""

from __future__ import annotations

from datetime import timedelta

from temporalio import workflow
from temporalio.exceptions import ApplicationError

with workflow.unsafe.imports_passed_through():
    from models import (
        ApprovalInput,
        ApprovalOutput,
        Decision,
        Participant,
        TaskCompleted,
        TaskResult,
        Verdict,
    )
    from policy import evaluate


@workflow.defn
class ApprovalWorkflow:
    """單一並簽節點的流程。正式版會是完整 DSL Interpreter 的一部分。"""

    def __init__(self) -> None:
        self._results: list[TaskResult] = []
        self._participants: list[Participant] = []
        self._task_ids: list[str] = []
        self._cancelled: list[str] = []

    @workflow.run
    async def run(self, inp: ApprovalInput) -> ApprovalOutput:
        # 1. 解析簽核人。Activity，因為要查資料庫。
        #    result_type 必須明確指定，否則 Temporal 回傳 dict 而非 dataclass。
        self._participants = await workflow.execute_activity(
            "resolve_approvers",
            inp.business_object,
            start_to_close_timeout=timedelta(seconds=30),
            result_type=list[Participant],
        )

        # 防禦：某些序列化路徑仍可能回傳 dict
        self._participants = [
            p if isinstance(p, Participant) else Participant(**p) for p in self._participants
        ]

        if not self._participants:
            # 正式版此處進 FAILED_RESOLUTION 並通知 admin
            raise ApplicationError(
                "簽核人解析結果為空，請確認部門主管或角色已指派人員",
                type="RESOLVE_EMPTY",
                non_retryable=True,
            )

        # 2. 建立 Human Task
        self._task_ids = await workflow.execute_activity(
            "create_human_tasks",
            args=[inp.instance_id, self._participants],
            start_to_close_timeout=timedelta(seconds=30),
        )

        # 3. 等待簽核結果或逾期
        deadline = None
        if inp.timeout_seconds:
            deadline = workflow.now() + timedelta(seconds=inp.timeout_seconds)

        def done() -> bool:
            return self._verdict(inp, deadline) is not Verdict.WAIT

        if inp.timeout_seconds:
            try:
                await workflow.wait_condition(
                    done, timeout=timedelta(seconds=inp.timeout_seconds)
                )
            except TimeoutError:
                pass  # 逾期由 _verdict 判斷
        else:
            await workflow.wait_condition(done)

        verdict = self._verdict(inp, deadline)

        # 4. 取消尚未完成的 Task
        decided = {r.task_id for r in self._results}
        pending = [t for t in self._task_ids if t not in decided]
        if pending:
            await workflow.execute_activity(
                "cancel_human_tasks",
                pending,
                start_to_close_timeout=timedelta(seconds=30),
            )
            self._cancelled = pending

        return ApprovalOutput(
            verdict=verdict,
            results=self._results,
            participants=self._participants,
            cancelled_task_ids=self._cancelled,
        )

    def _verdict(self, inp: ApprovalInput, deadline) -> Verdict:
        return evaluate(
            results=self._results,
            total_participants=len(self._participants),
            policy=inp.join,
            now=workflow.now(),
            deadline=deadline,
        )

    @workflow.signal
    async def task_completed(self, s: TaskCompleted) -> None:
        """使用者在前端按下同意或退回，Rust API 驗證後送出此 Signal。"""
        if any(r.task_id == s.task_id for r in self._results):
            return  # 重複 Signal，忽略
        self._results.append(
            TaskResult(
                task_id=s.task_id,
                participant_id=s.participant_id,
                decision=s.decision,
                comment=s.comment,
                decided_at=workflow.now(),
            )
        )

    @workflow.query
    def current_state(self) -> dict:
        """供 Monitor 查詢即時狀態，不影響流程。"""
        decided = {r.task_id for r in self._results}
        return {
            "participants": [p.display for p in self._participants],
            "completed": len(self._results),
            "total": len(self._participants),
            "pending_task_ids": [t for t in self._task_ids if t not in decided],
            "decisions": [
                {"participant": r.participant_id, "decision": r.decision.value}
                for r in self._results
            ],
        }
