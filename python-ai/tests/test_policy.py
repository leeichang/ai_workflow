"""
Approval Policy Engine 測試

完成策略與結果策略的組合矩陣。這些規則決定「幾個人簽了才算數」，
錯了會讓單據在不該通過時通過——是業務上最嚴重的一類缺陷。
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone

import pytest

from dsl.models import Completion, Decision, JoinPolicy, ResultPolicy, TaskResult, Verdict
from policy.engine import evaluate

NOW = datetime(2026, 9, 15, 12, 0, tzinfo=timezone.utc)


def result(participant: str, decision: str) -> TaskResult:
    return TaskResult(
        task_id=f"task-{participant}",
        participant_id=participant,
        decision=decision,
    )


def approvals(n: int) -> list[TaskResult]:
    return [result(f"u{i}", "APPROVE") for i in range(n)]


class Test完成策略_ALL:
    def policy(self) -> JoinPolicy:
        return JoinPolicy(completion=Completion.ALL, result=ResultPolicy.ALL_SUCCESS)

    def test_全員核准才前進(self):
        assert evaluate(approvals(3), 3, self.policy()) is Verdict.CONTINUE

    def test_尚有人未回應時等待(self):
        assert evaluate(approvals(2), 3, self.policy()) is Verdict.WAIT

    def test_無人回應時等待(self):
        assert evaluate([], 3, self.policy()) is Verdict.WAIT

    def test_全員回應但有人否決時退回(self):
        results = approvals(2) + [result("u2", "REJECT")]
        assert evaluate(results, 3, self.policy()) is Verdict.REJECT


class Test完成策略_ANY:
    def policy(self) -> JoinPolicy:
        return JoinPolicy(completion=Completion.ANY, result=ResultPolicy.ALL_SUCCESS)

    def test_一人核准即前進(self):
        assert evaluate(approvals(1), 3, self.policy()) is Verdict.CONTINUE

    def test_無人回應時等待(self):
        assert evaluate([], 3, self.policy()) is Verdict.WAIT


class Test完成策略_N_OF_M:
    def policy(self, n: int) -> JoinPolicy:
        return JoinPolicy(
            completion=Completion.N_OF_M, n=n, result=ResultPolicy.ALL_SUCCESS
        )

    def test_達到門檻即前進(self):
        assert evaluate(approvals(2), 5, self.policy(2)) is Verdict.CONTINUE

    def test_未達門檻時等待(self):
        assert evaluate(approvals(1), 5, self.policy(2)) is Verdict.WAIT

    def test_門檻只計核准不計否決(self):
        # 兩人回應但一人否決，核准數只有 1，未達門檻 2
        results = [result("u0", "APPROVE"), result("u1", "REJECT")]
        assert evaluate(results, 5, self.policy(2)) is Verdict.WAIT

    def test_缺少_n_時明確拋錯(self):
        policy = JoinPolicy(completion=Completion.N_OF_M, n=None)
        with pytest.raises(ValueError, match="n 必填"):
            evaluate(approvals(1), 3, policy)


class Test結果策略_ANY_REJECT:
    def policy(self) -> JoinPolicy:
        return JoinPolicy(completion=Completion.ALL, result=ResultPolicy.ANY_REJECT)

    def test_一人否決立即退回不等其他人(self):
        # 這是並簽最重要的語意。三人會簽，第一人否決時
        # 不該讓另外兩人白跑一趟。
        results = [result("u0", "REJECT")]
        assert evaluate(results, 3, self.policy()) is Verdict.REJECT

    def test_結果策略優先於完成策略(self):
        # completion=ALL 尚未滿足（只有 1/3 回應），
        # 但 ANY_REJECT 讓它直接退回而非 WAIT
        results = [result("u0", "REJECT")]
        verdict = evaluate(results, 3, self.policy())
        assert verdict is Verdict.REJECT
        assert verdict is not Verdict.WAIT

    def test_全員核准時前進(self):
        assert evaluate(approvals(3), 3, self.policy()) is Verdict.CONTINUE


class Test結果策略_MAJORITY:
    def policy(self) -> JoinPolicy:
        return JoinPolicy(completion=Completion.ALL, result=ResultPolicy.MAJORITY)

    def test_過半核准時前進(self):
        results = approvals(2) + [result("u2", "REJECT")]
        assert evaluate(results, 3, self.policy()) is Verdict.CONTINUE

    def test_未過半時退回(self):
        results = [result("u0", "APPROVE")] + [
            result("u1", "REJECT"),
            result("u2", "REJECT"),
        ]
        assert evaluate(results, 3, self.policy()) is Verdict.REJECT

    def test_剛好一半不算過半(self):
        # 四人兩票贊成兩票反對。2*2 > 4 為假，因此退回。
        # 邊界值刻意測試：「過半」與「至少一半」在真實專案中常有爭議。
        results = approvals(2) + [result("u2", "REJECT"), result("u3", "REJECT")]
        assert evaluate(results, 4, self.policy()) is Verdict.REJECT

    def test_分母是全體而非已回應者(self):
        # 三人會簽只有一人核准時，若分母用「已回應者」，
        # 1 > 0.5 會被誤判為過半而放行。
        policy = JoinPolicy(completion=Completion.ANY, result=ResultPolicy.MAJORITY)
        assert evaluate(approvals(1), 3, policy) is Verdict.REJECT


class Test逾時:
    def policy(self) -> JoinPolicy:
        return JoinPolicy(completion=Completion.ALL, result=ResultPolicy.ALL_SUCCESS)

    def test_未到期時等待(self):
        deadline = NOW + timedelta(days=1)
        verdict = evaluate(approvals(1), 3, self.policy(), now=NOW, deadline=deadline)
        assert verdict is Verdict.WAIT

    def test_已到期且未滿足時逾時(self):
        deadline = NOW - timedelta(seconds=1)
        verdict = evaluate(approvals(1), 3, self.policy(), now=NOW, deadline=deadline)
        assert verdict is Verdict.TIMEOUT

    def test_已到期但條件已滿足時仍前進(self):
        # 條件先滿足就該前進，不該因為稍後才檢查時間而變成逾時
        deadline = NOW - timedelta(seconds=1)
        verdict = evaluate(approvals(3), 3, self.policy(), now=NOW, deadline=deadline)
        assert verdict is Verdict.CONTINUE

    def test_沒有_deadline_時不會逾時(self):
        verdict = evaluate(approvals(1), 3, self.policy(), now=NOW, deadline=None)
        assert verdict is Verdict.WAIT

    def test_沒有_now_時不會逾時(self):
        # now 由呼叫端提供以保持決定性。沒提供時不判斷逾時，
        # 而不是偷偷呼叫 datetime.now()。
        deadline = NOW - timedelta(days=99)
        verdict = evaluate(approvals(1), 3, self.policy(), now=None, deadline=deadline)
        assert verdict is Verdict.WAIT


class Test純函數性質:
    def test_不修改輸入(self):
        results = approvals(2)
        snapshot = [
            (r.task_id, r.participant_id, r.decision, r.comment) for r in results
        ]

        evaluate(results, 3, JoinPolicy())

        assert [
            (r.task_id, r.participant_id, r.decision, r.comment) for r in results
        ] == snapshot

    def test_同樣輸入必得同樣輸出(self):
        # Workflow 重放時結果不同會被判定為非決定性而失敗
        results = approvals(2) + [result("u2", "REJECT")]
        policy = JoinPolicy(completion=Completion.ALL, result=ResultPolicy.MAJORITY)

        verdicts = {evaluate(results, 3, policy, now=NOW) for _ in range(50)}
        assert len(verdicts) == 1


class Test列舉還原:
    def test_字串會被還原成_enum(self):
        # Temporal 以 JSON 傳遞 dataclass，Enum 欄位回來可能是字串
        policy = JoinPolicy(completion="ANY", result="ANY_REJECT")
        assert policy.completion is Completion.ANY
        assert policy.result is ResultPolicy.ANY_REJECT

    def test_字元陣列也會被還原(self):
        # PoC 階段踩過：ALL 被拆成 ['A','L','L']
        policy = JoinPolicy(completion=["A", "L", "L"])
        assert policy.completion is Completion.ALL

    def test_decision_同樣適用(self):
        r = TaskResult(task_id="t", participant_id="u", decision="APPROVE")
        assert r.decision is Decision.APPROVE
