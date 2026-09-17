"""
人工待辦與參與者解析 Activity

這些動作要寫資料庫，因此必須在 Activity 內執行，不能在 Workflow 內。
實作方式是呼叫 Rust 的 internal API，而非直接連資料庫：

  - 租戶隔離、稽核、權限判定都在 Rust 那一層，繞過它就等於繞過 RLS。
  - 業務規則只該有一份。Python 自己接資料庫遲早會與 Rust 的行為漂移。

internal API 不對外開放，以共享密鑰認證（X-Internal-Token）。
它信任呼叫端給的 tenant_id，因此絕不能暴露到公網。
"""

from __future__ import annotations

import os
from typing import Any

import httpx
from temporalio import activity
from temporalio.exceptions import ApplicationError

API_BASE = os.getenv("INTERNAL_API_BASE", "http://localhost:3001")
INTERNAL_TOKEN = os.getenv("INTERNAL_API_TOKEN", "dev-internal-token")
TIMEOUT = httpx.Timeout(20.0)


def _headers(tenant_id: str) -> dict[str, str]:
    return {
        "X-Internal-Token": INTERNAL_TOKEN,
        "X-Tenant-Id": tenant_id,
        "Content-Type": "application/json",
    }


async def _post(path: str, tenant_id: str, body: dict[str, Any]) -> Any:
    async with httpx.AsyncClient(timeout=TIMEOUT) as client:
        response = await client.post(
            f"{API_BASE}{path}", json=body, headers=_headers(tenant_id)
        )

    if response.status_code >= 500:
        # 5xx 可重試，讓 Temporal 的 retry policy 處理
        raise ApplicationError(
            f"internal API {path} 回 {response.status_code}：{response.text[:200]}"
        )

    if response.status_code >= 400:
        # 4xx 是請求本身有問題，重試不會變好
        raise ApplicationError(
            f"internal API {path} 拒絕請求（{response.status_code}）：{response.text[:200]}",
            non_retryable=True,
        )

    if not response.content:
        return None
    return response.json()


@activity.defn(name="resolve_participants")
async def resolve_participants(req: dict) -> list[dict]:
    """解析簽核人

    resolver 的型別（manager_of、role、external_contacts 等）
    由 Rust 端解讀，因為它掌握組織結構與角色指派。
    """
    result = await _post(
        "/internal/participants/resolve",
        req["tenant_id"],
        {
            "instance_id": req["instance_id"],
            "spec": req["spec"],
            "business_object": req.get("business_object") or {},
        },
    )
    return result or []


@activity.defn(name="create_human_tasks")
async def create_human_tasks(req: dict) -> list[str]:
    """建立待辦，回傳 task_id 清單

    順序必須與 participants 一致，Workflow 會用索引對應。
    """
    result = await _post(
        "/internal/human-tasks",
        req["tenant_id"],
        {
            "instance_id": req["instance_id"],
            "node_id": req["node_id"],
            "node_label": req.get("node_label"),
            "participants": req["participants"],
            "participant_kind": req.get("participant_kind", "internal"),
            "form_key": req.get("form_key"),
            "due_at": req.get("due_at"),
        },
    )
    return (result or {}).get("task_ids", [])


@activity.defn(name="cancel_human_tasks")
async def cancel_human_tasks(req: dict) -> None:
    """取消未完成的待辦

    並簽有人否決後，其餘的待辦必須取消，
    否則收件匣會留下永遠點不動的項目。
    """
    if not req.get("task_ids"):
        return

    await _post(
        "/internal/human-tasks/cancel",
        req["tenant_id"],
        {"task_ids": req["task_ids"], "reason": req.get("reason", "")},
    )


@activity.defn(name="run_action")
async def run_action(req: dict) -> dict:
    """執行系統動作

    idempotency_key 由 Workflow 產生且在重試間保持一致，
    Rust 端據此去重——重試不會建立兩張訂單。
    """
    result = await _post(
        "/internal/actions/run",
        req["tenant_id"],
        {
            "instance_id": req["instance_id"],
            "node_id": req["node_id"],
            "action": req["action"],
            "input_mapping": req.get("input_mapping") or {},
            "business_object": req.get("business_object") or {},
            "idempotency_key": req["idempotency_key"],
        },
    )
    return result or {}


@activity.defn(name="send_notification")
async def send_notification(req: dict) -> None:
    await _post(
        "/internal/notifications",
        req["tenant_id"],
        {
            "instance_id": req["instance_id"],
            "node_id": req["node_id"],
            "channel": req.get("channel") or ["email"],
            "to": req.get("to") or [],
            "template_key": req.get("template_key"),
            "business_object": req.get("business_object") or {},
            # 提醒與一般通知的信件內容不同，kind 決定用哪個標題。
            # 漏傳的話提醒信會顯示成通用的「流程通知」，
            # 收信的人不知道是催簽核還是別的事。
            "kind": req.get("kind"),
            "remaining": req.get("remaining"),
        },
    )


@activity.defn(name="report_instance_finished")
async def report_instance_finished(req: dict) -> None:
    """回報流程結束

    Temporal 知道流程結束了，但業務資料庫不知道。
    收件匣與報表查的是資料庫，不回報的話單據會永遠顯示「進行中」。
    """
    await _post(
        f"/internal/instances/{req['instance_id']}/finish",
        req["tenant_id"],
        {
            "status": req["status"],
            "output": req.get("output"),
            "error": req.get("error"),
        },
    )


ALL_ACTIVITIES = [
    resolve_participants,
    create_human_tasks,
    cancel_human_tasks,
    run_action,
    send_notification,
    report_instance_finished,
]
