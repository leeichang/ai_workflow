"""
DSL 與執行期的資料模型

對應 schemas/workflow-dsl.schema.json。用 dataclass 而非 Pydantic：
Temporal 的沙箱會攔截動態匯入，Pydantic 的模型建構在 Workflow 內
容易踩到限制，而我們的需求只是「把 dict 變成有型別的物件」。

Enum 欄位一律經 _to_enum 正規化。Temporal 以 JSON 傳遞 dataclass，
反序列化時 Enum 可能回來是字串，甚至被誤拆成字元陣列
（"ALL" → ['A','L','L']）。PoC 階段踩過這個坑。
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Any


def _to_enum(enum_cls, value):
    """把字串或字元陣列還原成 Enum 成員"""
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


class TimeoutPolicy(str, Enum):
    WAIT = "WAIT"
    ESCALATE = "ESCALATE"
    AUTO_APPROVE = "AUTO_APPROVE"
    AUTO_REJECT = "AUTO_REJECT"


class InstanceStatus(str, Enum):
    RUNNING = "RUNNING"
    COMPLETED = "COMPLETED"
    REJECTED = "REJECTED"
    CANCELLED = "CANCELLED"
    FAILED = "FAILED"


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
    """Approver Resolver 的輸出，由 Rust API 回傳"""

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
    decided_at: str | None = None

    def __post_init__(self) -> None:
        self.decision = _to_enum(Decision, self.decision)


@dataclass
class TaskCompleted:
    """task_completed Signal 的 payload

    node_id 必填：流程可能因退回而重跑同一節點，
    沒有它無法判斷這個決策屬於哪一輪。
    """

    task_id: str
    node_id: str
    participant_id: str
    decision: Decision
    comment: str = ""

    def __post_init__(self) -> None:
        self.decision = _to_enum(Decision, self.decision)


@dataclass
class InstanceInput:
    """Workflow 的輸入

    dsl 整份帶入而非只帶 version_id：Workflow 啟動後就固定用這一份，
    即使流程定義之後又發布新版，進行中的實例也不受影響。
    簽核到一半規則改變是不可接受的。
    """

    tenant_id: str
    instance_id: str
    workflow_version_id: str
    dsl: dict[str, Any]
    business_object: dict[str, Any] = field(default_factory=dict)


@dataclass
class InstanceResult:
    status: InstanceStatus
    """走過的節點，依序。供稽核與除錯。"""
    path: list[str] = field(default_factory=list)
    output: dict[str, Any] = field(default_factory=dict)
    error: str | None = None

    def __post_init__(self) -> None:
        self.status = _to_enum(InstanceStatus, self.status)


@dataclass
class CurrentState:
    """current_state Query 的回傳

    並行時同時有多個節點在等待，因此 active_nodes 是清單。
    current_node 保留給單線流程與既有呼叫端——並行時它是
    active_nodes 的第一個，語意上「不精確但不會錯到哪」。
    要正確判斷流程位置請用 active_nodes。
    """

    current_node: str | None
    path: list[str] = field(default_factory=list)
    pending_task_ids: list[str] = field(default_factory=list)
    results: list[dict[str, Any]] = field(default_factory=list)
    business_object: dict[str, Any] = field(default_factory=dict)
    """正在等待的節點。單線流程時只有一個，並行時有多個。"""
    active_nodes: list[str] = field(default_factory=list)
