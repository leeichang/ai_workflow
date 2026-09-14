"""
Approval Policy Engine 表格式測試

對應 docs/系統規劃/03_功能規劃.md 5.2 節的矩陣。
正式版 python-ai/tests/test_policy.py 必須包含同一組案例。
"""

from datetime import datetime, timedelta, timezone

import pytest
from models import Completion, Decision, JoinPolicy, ResultPolicy, TaskResult, Verdict
from policy import evaluate

NOW = datetime(2026, 9, 14, 12, 0, tzinfo=timezone.utc)


def results(*decisions: str) -> list[TaskResult]:
    """'A' = APPROVE, 'R' = REJECT"""
    out = []
    for i, d in enumerate(decisions):
        out.append(
            TaskResult(
                task_id=f"t{i}",
                participant_id=f"u{i}",
                decision=Decision.APPROVE if d == "A" else Decision.REJECT,
                decided_at=NOW,
            )
        )
    return out


@pytest.mark.parametrize(
    "name,decisions,total,completion,n,result_policy,expected",
    [
        # ALL + ALL_SUCCESS
        ("3人2同意1未回", "AA", 3, Completion.ALL, None, ResultPolicy.ALL_SUCCESS, Verdict.WAIT),
        ("3人全同意", "AAA", 3, Completion.ALL, None, ResultPolicy.ALL_SUCCESS, Verdict.CONTINUE),
        ("3人全回應含拒絕", "AAR", 3, Completion.ALL, None, ResultPolicy.ALL_SUCCESS, Verdict.REJECT),
        # ALL + ANY_REJECT：一人拒絕立即退回，不等其他人
        ("3人1拒絕2未回", "R", 3, Completion.ALL, None, ResultPolicy.ANY_REJECT, Verdict.REJECT),
        ("3人2同意1未回", "AA", 3, Completion.ALL, None, ResultPolicy.ANY_REJECT, Verdict.WAIT),
        ("3人全同意", "AAA", 3, Completion.ALL, None, ResultPolicy.ANY_REJECT, Verdict.CONTINUE),
        # ANY
        ("3人1同意即通過", "A", 3, Completion.ANY, None, ResultPolicy.ALL_SUCCESS, Verdict.CONTINUE),
        ("3人1拒絕", "R", 3, Completion.ANY, None, ResultPolicy.ALL_SUCCESS, Verdict.REJECT),
        # N_OF_M
        ("4人需2同意已2", "AA", 4, Completion.N_OF_M, 2, ResultPolicy.ALL_SUCCESS, Verdict.CONTINUE),
        ("4人需3同意僅2", "AA", 4, Completion.N_OF_M, 3, ResultPolicy.ALL_SUCCESS, Verdict.WAIT),
        ("5人需3同意已3含1拒絕", "AARA", 5, Completion.N_OF_M, 3, ResultPolicy.ANY_REJECT, Verdict.REJECT),
        # MAJORITY
        ("5人3同意過半", "AAA", 5, Completion.ANY, None, ResultPolicy.MAJORITY, Verdict.CONTINUE),
        ("5人2同意未過半", "AA", 5, Completion.ANY, None, ResultPolicy.MAJORITY, Verdict.REJECT),
        # 邊界
        ("單人同意", "A", 1, Completion.ALL, None, ResultPolicy.ALL_SUCCESS, Verdict.CONTINUE),
        ("無人回應", "", 3, Completion.ALL, None, ResultPolicy.ALL_SUCCESS, Verdict.WAIT),
    ],
)
def test_policy_matrix(name, decisions, total, completion, n, result_policy, expected):
    verdict = evaluate(
        results=results(*decisions),
        total_participants=total,
        policy=JoinPolicy(completion=completion, n=n, result=result_policy),
        now=NOW,
        deadline=None,
    )
    assert verdict is expected, f"{name}: 期望 {expected}，實際 {verdict}"


def test_timeout_only_when_not_satisfied():
    """逾期判斷在完成條件之後。已滿足條件就不該回 TIMEOUT。"""
    past = NOW - timedelta(hours=1)

    # 未滿足且已逾期 → TIMEOUT
    assert evaluate(
        results=results("A"),
        total_participants=3,
        policy=JoinPolicy(completion=Completion.ALL),
        now=NOW,
        deadline=past,
    ) is Verdict.TIMEOUT

    # 已滿足即使逾期 → CONTINUE，不應被逾期蓋過
    assert evaluate(
        results=results("A", "A", "A"),
        total_participants=3,
        policy=JoinPolicy(completion=Completion.ALL),
        now=NOW,
        deadline=past,
    ) is Verdict.CONTINUE


def test_no_deadline_never_times_out():
    assert evaluate(
        results=results("A"),
        total_participants=3,
        policy=JoinPolicy(completion=Completion.ALL),
        now=NOW,
        deadline=None,
    ) is Verdict.WAIT


def test_n_of_m_requires_n():
    with pytest.raises(ValueError, match="n 必填"):
        evaluate(
            results=results("A"),
            total_participants=3,
            policy=JoinPolicy(completion=Completion.N_OF_M, n=None),
            now=NOW,
        )


def test_pure_function_no_side_effects():
    """同樣輸入必須產生同樣輸出，且不修改輸入。"""
    rs = results("A", "A")
    policy = JoinPolicy(completion=Completion.ALL)
    snapshot = [(r.task_id, r.decision) for r in rs]

    first = evaluate(results=rs, total_participants=3, policy=policy, now=NOW)
    second = evaluate(results=rs, total_participants=3, policy=policy, now=NOW)

    assert first is second
    assert [(r.task_id, r.decision) for r in rs] == snapshot
