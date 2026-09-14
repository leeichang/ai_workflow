"""
PoC 用的資料模型。

正式版會由 schemas/*.json 產生 Pydantic 型別（任務 1.4 之前）。
此處手寫最小集合，只為驗證 Temporal 行為。
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum


def _to_enum(enum_cls, value):
    """
    把字串或字元陣列還原成 Enum 成員。

    Temporal 以 JSON 傳遞 dataclass。反序列化時 Enum 欄位可能回來是字串，
    甚至被誤拆成字元陣列（例如 "ALL" → ['A','L','L']）。所有含 Enum 的
    dataclass 都在 __post_init__ 呼叫此函式正規化。

    正式版改用 Pydantic 由 schemas/*.json 產生型別，此問題自然消失。
    """
    if isinstance(value, enum_cls):
        return value
    if isinstance(value, (list, tuple)):
        value = "".join(value)
    return enum_cls(str(value))


class Decision(str, Enum):
    APPROVE = "APPROVE"
    REJECT = "REJECT"


class Verdict(str, Enum):
    """Approval Policy Engine 的輸出"""
    CONTINUE = "CONTINUE"
    WAIT = "WAIT"
    REJECT = "REJECT"
    TIMEOUT = "TIMEOUT"


class Completion(str, Enum):
    ALL = "ALL"
    ANY = "ANY"
    N_OF_M = "N_OF_M"


class ResultPolicy(str, Enum):
    ALL_SUCCESS = "ALL_SUCCESS"
    ANY_REJECT = "ANY_REJECT"
    MAJORITY = "MAJORITY"


@dataclass
class JoinPolicy:
    completion: Completion = Completion.ALL
    n: int | None = None
    result: ResultPolicy = ResultPolicy.ALL_SUCCESS

    def __post_init__(self) -> None:
        self.completion = _to_enum(Completion, self.completion)
        self.result = _to_enum(ResultPolicy, self.result)


@dataclass
class Participant:
    """Approver Resolver 的輸出。正式版由 Rust API 回傳。"""
    id: str
    display: str
    role: str | None = None
    kind: str = "internal"


@dataclass
class TaskResult:
    task_id: str
    participant_id: str
    decision: Decision
    comment: str = ""
    decided_at: datetime | None = None

    def __post_init__(self) -> None:
        self.decision = _to_enum(Decision, self.decision)


@dataclass
class TaskCompleted:
    """Signal payload"""
    task_id: str
    participant_id: str
    decision: Decision
    comment: str = ""

    def __post_init__(self) -> None:
        self.decision = _to_enum(Decision, self.decision)


@dataclass
class ApprovalInput:
    """Workflow 輸入"""
    instance_id: str
    business_object: dict
    join: JoinPolicy = field(default_factory=JoinPolicy)
    timeout_seconds: int | None = None

    def __post_init__(self) -> None:
        # 巢狀 dataclass 同樣可能被還原成 dict
        if isinstance(self.join, dict):
            self.join = JoinPolicy(**self.join)


@dataclass
class ApprovalOutput:
    verdict: Verdict
    results: list[TaskResult]
    participants: list[Participant]
    cancelled_task_ids: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        self.verdict = _to_enum(Verdict, self.verdict)
        self.results = [
            r if isinstance(r, TaskResult) else TaskResult(**r) for r in self.results
        ]
        self.participants = [
            p if isinstance(p, Participant) else Participant(**p) for p in self.participants
        ]
