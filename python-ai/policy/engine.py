"""
Approval Policy Engine

純函數，無 IO，無時間呼叫（now 由參數傳入）。
因此可在 Temporal Workflow 內直接呼叫而不破壞決定性——
Workflow 重放時同樣的輸入必然得到同樣的輸出。

若改成查資料庫或呼叫 datetime.now()，重放時結果可能與
第一次執行不同，Temporal 會判定為非決定性並讓流程失敗。
"""

from __future__ import annotations

from datetime import datetime

from dsl.models import Completion, Decision, JoinPolicy, ResultPolicy, TaskResult, Verdict


def evaluate(
    results: list[TaskResult],
    total_participants: int,
    policy: JoinPolicy,
    now: datetime | None = None,
    deadline: datetime | None = None,
) -> Verdict:
    """
    判斷簽核節點目前應該繼續、等待、退回，還是逾時。

    參數
        results             已完成的簽核結果
        total_participants  參與者總數（含尚未回應者）
        policy              完成策略與結果策略
        now                 當前時間，由呼叫端提供以保持決定性
        deadline            逾期時間，None 表示不設限

    回傳
        CONTINUE  條件滿足，流程前進
        WAIT      尚未滿足，繼續等待
        REJECT    應走退回路徑
        TIMEOUT   已逾期且仍未滿足
    """
    approved = sum(1 for r in results if r.decision is Decision.APPROVE)
    rejected = sum(1 for r in results if r.decision is Decision.REJECT)
    completed = len(results)

    # 結果策略優先於完成策略：任一拒絕即退回，不等其他人。
    # 這是並簽最重要的語意——三人會簽，第一人否決時
    # 不該讓另外兩人白跑一趟。
    if policy.result is ResultPolicy.ANY_REJECT and rejected > 0:
        return Verdict.REJECT

    satisfied = _completion_satisfied(
        completion=policy.completion,
        n=policy.n,
        completed=completed,
        approved=approved,
        total=total_participants,
    )

    if satisfied:
        return _apply_result_policy(
            policy.result,
            approved=approved,
            rejected=rejected,
            total=total_participants,
        )

    # 未滿足，檢查是否逾期
    if deadline is not None and now is not None and now >= deadline:
        return Verdict.TIMEOUT

    return Verdict.WAIT


def _completion_satisfied(
    *, completion: Completion, n: int | None, completed: int, approved: int, total: int
) -> bool:
    if completion is Completion.ALL:
        return completed >= total
    if completion is Completion.ANY:
        return completed >= 1
    if completion is Completion.N_OF_M:
        if n is None:
            raise ValueError("completion=N_OF_M 時 n 必填")
        return approved >= n
    raise ValueError(f"未知的 completion 策略：{completion}")


def _apply_result_policy(
    result: ResultPolicy, *, approved: int, rejected: int, total: int
) -> Verdict:
    if result is ResultPolicy.ALL_SUCCESS:
        return Verdict.CONTINUE if rejected == 0 else Verdict.REJECT
    if result is ResultPolicy.ANY_REJECT:
        # 到這裡代表沒有拒絕，否則前面已回 REJECT
        return Verdict.CONTINUE
    if result is ResultPolicy.MAJORITY:
        # 過半以「全體參與者」為分母，而非「已回應者」。
        # 否則三人會簽只有一人回覆時，1 > 0 會被誤判為過半。
        return Verdict.CONTINUE if approved * 2 > total else Verdict.REJECT
    raise ValueError(f"未知的 result 策略：{result}")
