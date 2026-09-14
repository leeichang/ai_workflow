"""
Activity 實作

PoC 中以記憶體模擬。正式版：
  - resolve_approvers   → HTTP POST /internal/resolver/resolve（Rust）
  - create_human_tasks  → HTTP POST /internal/human-tasks（Rust）
  - cancel_human_tasks  → HTTP POST /internal/human-tasks/cancel（Rust）
"""

from __future__ import annotations

from temporalio import activity

from models import Participant

# 模擬資料庫。正式版查 PostgreSQL。
TASK_STORE: dict[str, dict] = {}
CALL_LOG: list[tuple[str, object]] = []


@activity.defn(name="resolve_approvers")
async def resolve_approvers(business_object: dict) -> list[Participant]:
    """
    簽核人解析。對應 composite resolver：
        部門主管 + 財務主管 + （金額 > 100 萬時加 CFO）

    正式版由 Rust 的 Approver Resolver 執行，此處以硬編碼模擬其輸出。
    去重邏輯（同一人被多個子項命中只產生一個 task）也在 Rust 端。
    """
    CALL_LOG.append(("resolve_approvers", business_object))

    total = business_object.get("total", 0)
    people = [
        Participant(id="u-mgr", display="王經理", role="department_manager"),
        Participant(id="u-fin", display="李財務", role="finance_manager"),
    ]
    if total > 1_000_000:
        people.append(Participant(id="u-cfo", display="陳財務長", role="cfo"))

    # 去重，模擬 Rust 端行為
    seen: set[str] = set()
    unique: list[Participant] = []
    for p in people:
        if p.id in seen:
            continue
        seen.add(p.id)
        unique.append(p)
    return unique


@activity.defn(name="create_human_tasks")
async def create_human_tasks(instance_id: str, participants: list[Participant]) -> list[str]:
    """建立待簽核任務，回傳 task_id 清單。"""
    CALL_LOG.append(("create_human_tasks", instance_id))
    # Activity 參數經 JSON 往返，可能是 dict
    participants = [p if isinstance(p, Participant) else Participant(**p) for p in participants]
    ids: list[str] = []
    for p in participants:
        task_id = f"{instance_id}:{p.id}"
        TASK_STORE[task_id] = {
            "instance_id": instance_id,
            "participant_id": p.id,
            "display": p.display,
            "status": "PENDING",
        }
        ids.append(task_id)
    return ids


@activity.defn(name="cancel_human_tasks")
async def cancel_human_tasks(task_ids: list[str]) -> int:
    """取消尚未完成的任務。CONTINUE 或 REJECT 後呼叫。"""
    CALL_LOG.append(("cancel_human_tasks", list(task_ids)))
    n = 0
    for tid in task_ids:
        t = TASK_STORE.get(tid)
        if t and t["status"] == "PENDING":
            t["status"] = "CANCELLED"
            n += 1
    return n


def reset() -> None:
    """測試之間清空狀態。"""
    TASK_STORE.clear()
    CALL_LOG.clear()


ALL_ACTIVITIES = [resolve_approvers, create_human_tasks, cancel_human_tasks]
