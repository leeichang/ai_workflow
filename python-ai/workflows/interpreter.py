"""
DSL Interpreter Workflow

走訪流程圖，依節點型別執行對應動作。這是整個平台的核心：
使用者在設計器畫出來的圖，由這裡真的跑起來。

Temporal 的決定性約束（違反會讓流程在重放時失敗）：
  - 不做 IO。查資料庫、呼叫外部系統一律走 Activity。
  - 不讀系統時鐘。時間用 workflow.now()，重放時回傳原始時間。
  - 不用亂數、不迭代 set、不依賴 dict 以外的無序結構。
  - Policy Engine 與表達式求值是純函數，可直接呼叫。

兩種控制流（見 schemas/README.md）：
  edges                正常前進路徑
  on_reject/on_failure 的 goto  退回路徑
節點走訪必須同時處理兩者，只看 edges 會讓退回節點永遠到不了。
"""

from __future__ import annotations

import asyncio
from datetime import timedelta
from typing import Any

from temporalio import workflow
from temporalio.common import RetryPolicy as TemporalRetry
from temporalio.exceptions import ApplicationError

with workflow.unsafe.imports_passed_through():
    from dsl.expression import ExpressionError, evaluate as eval_expression
    from dsl.models import (
        CurrentState,
        Decision,
        InstanceInput,
        InstanceResult,
        InstanceStatus,
        JoinPolicy,
        Participant,
        TaskCompleted,
        TaskResult,
        TimeoutPolicy,
        Verdict,
    )
    from dsl.duration import parse_iso_duration
    from policy.engine import evaluate as evaluate_policy

# 單一節點的預設 Activity 逾時。實際的人工等待由 wait_condition 處理，
# 這裡只管「呼叫 Rust API 建立待辦」這類短動作。
ACTIVITY_TIMEOUT = timedelta(seconds=30)

# 防呆上限。DSL 驗證（WF-E002 等）已擋掉大部分的無窮迴圈，
# 但退回路徑可以合法地讓流程反覆經過同一節點（改了再送、再被退），
# 靜態分析無法判斷次數。真的跑到上限代表流程設計有問題，
# 明確失敗比無聲空轉好。
MAX_STEPS = 500


@workflow.defn(name="DslInterpreter")
class DslInterpreter:
    def __init__(self) -> None:
        self._input: InstanceInput | None = None
        # 被要求快轉的節點。時間快轉不假造決策——
        # 它把等待視為已到期，走真正的逾時處理，
        # 這樣模擬看到的結果與正式環境逾時後一模一樣。
        self._skip_time: set[str] = set()
        self._nodes: dict[str, dict[str, Any]] = {}
        self._edges: list[list[Any]] = []

        self._path: list[str] = []
        self._business: dict[str, Any] = {}

        # 每個活躍節點各自的等待狀態。
        #
        # 並行時多個 human_approval 同時在等，決策必須落到正確的節點。
        # 早期版本用單一 self._current 與 self._results，那在單線流程
        # 沒問題，但並行時三個分支會共用同一組結果——財務的核准會被
        # 法務節點看見，policy engine 立刻誤判為完成。
        self._waits: dict[str, _NodeWait] = {}

        self._cancelled = False
        self._cancel_reason = ""

    # ── 主流程 ──────────────────────────────────────────

    @workflow.run
    async def run(self, inp: InstanceInput) -> InstanceResult:
        self._input = inp
        self._business = dict(inp.business_object or {})

        dsl = inp.dsl or {}
        self._nodes = {n["id"]: n for n in dsl.get("nodes", [])}
        self._edges = [list(e) for e in dsl.get("edges", [])]

        start = self._find_start()
        if start is None:
            return InstanceResult(
                status=InstanceStatus.FAILED,
                path=[],
                error="流程缺少 trigger 節點",
            )

        try:
            result = await self._walk(start)
        except asyncio.CancelledError:
            # 取消必須明確處理，否則流程以「失敗」結束且不執行清理。
            # 清理最重要的一步是取消待辦——否則使用者的收件匣會留下
            # 永遠點不動的項目。
            await self._cancel_pending(None, "流程已取消")
            result = InstanceResult(
                status=InstanceStatus.CANCELLED,
                path=self._path,
                error=self._cancel_reason or "流程已取消",
            )
        except ApplicationError as e:
            await self._cancel_pending(None, "流程失敗")
            result = InstanceResult(
                status=InstanceStatus.FAILED,
                path=self._path,
                error=str(e),
            )

        await self._report_finished(result)
        return result

    async def _report_finished(self, result: InstanceResult) -> None:
        """把最終狀態寫回業務資料庫

        Temporal 知道流程結束了，但收件匣與報表查的是 PostgreSQL。
        不回報的話單據會永遠顯示「進行中」。

        **無限重試，不放棄**（2026-09-17 修訂，見決議 D-06）。

        先前的做法是重試 5 次後記 warning 就算了，理由寫著
        「可由對帳程序修正」——但那個對帳程序從未實作，
        而且事後補本來就比當下寫成功脆弱：
        補的時候要重新判斷該寫什麼，而那個判斷可能也是錯的。

        無限重試的代價是資料庫長期斷線時流程停在這裡。
        但那本來就是系統無法運作的狀態，在 Temporal UI 看得到
        卡住的流程，比資料靜默不一致好得多。
        """
        await workflow.execute_activity(
            "report_instance_finished",
            args=[
                {
                    "tenant_id": self._input.tenant_id,
                    "instance_id": self._input.instance_id,
                    "status": result.status.value,
                    "output": result.output,
                    "error": result.error,
                }
            ],
            start_to_close_timeout=ACTIVITY_TIMEOUT,
            # 不設 maximum_attempts：Temporal 以指數退避持續重試
            retry_policy=TemporalRetry(),
        )

    async def _walk(self, start: str, stop_at: str | None = None) -> InstanceResult:
        """走訪流程圖

        stop_at 供並行分支使用：走到該節點（join）就停下來回報，
        由 parallel 節點統一判定。None 表示走到流程結束為止。
        """
        node_id: str | None = start

        for _ in range(MAX_STEPS):
            if node_id is None:
                # 沒有後繼節點。DSL 驗證要求所有路徑通往 end，
                # 走到這裡代表定義有問題，但流程已經跑了一半，
                # 以 FAILED 結束並記錄位置比靜默停止有用。
                return InstanceResult(
                    status=InstanceStatus.FAILED,
                    path=self._path,
                    error=f"節點 {self._path[-1] if self._path else '?'} 沒有後繼節點",
                )

            if node_id == stop_at:
                # 分支抵達匯合點。用 RUNNING 表示「這一段走完了，
                # 但流程還沒結束」——真正的結束由 parallel 節點決定。
                return InstanceResult(status=InstanceStatus.RUNNING, path=self._path)

            node = self._nodes.get(node_id)
            if node is None:
                return InstanceResult(
                    status=InstanceStatus.FAILED,
                    path=self._path,
                    error=f"找不到節點 {node_id}",
                )

            self._path.append(node_id)

            outcome = await self._execute(node, stop_at=stop_at)

            if outcome.finished is not None:
                return outcome.finished
            node_id = outcome.next_node

        return InstanceResult(
            status=InstanceStatus.FAILED,
            path=self._path,
            error=f"超過 {MAX_STEPS} 步仍未結束，流程可能有無窮迴圈",
        )

    async def _execute(
        self, node: dict[str, Any], stop_at: str | None = None
    ) -> "_Outcome":
        kind = node.get("type")

        if kind == "trigger":
            return _Outcome(next_node=self._next_of(node["id"]))

        if kind == "condition":
            return _Outcome(next_node=await self._run_condition(node))

        if kind in ("human_approval", "human_task"):
            return await self._run_human(node)

        if kind == "action":
            return await self._run_action(node)

        if kind == "notification":
            return await self._run_notification(node)

        if kind == "parallel":
            return await self._run_parallel(node)

        if kind == "join":
            # 正常情況下 join 由 parallel 消化，不會單獨走到這裡。
            # 走到代表 DSL 有 join 卻沒有對應的 parallel。
            raise ApplicationError(
                f"節點 {node['id']} 是 join 但沒有對應的 parallel",
                type="OrphanJoin",
                non_retryable=True,
            )

        if kind == "end":
            return _Outcome(finished=self._finish(node))

        # 明確失敗而非靜默跳過：靜默跳過會讓流程看似成功，實際少做了事。
        raise ApplicationError(
            f"尚未支援的節點型別：{kind}（節點 {node.get('id')}）",
            type="UnsupportedNodeType",
            non_retryable=True,
        )

    # ── 各型別節點 ──────────────────────────────────────

    async def _run_condition(self, node: dict[str, Any]) -> str | None:
        expression = node.get("expression") or ""
        try:
            result = eval_expression(expression, self._context(node["id"]))
        except ExpressionError as e:
            raise ApplicationError(
                f"節點 {node['id']} 的條件無法求值：{e}",
                type="InvalidExpression",
                non_retryable=True,
            ) from e

        branch = self._next_of(node["id"], when=result)
        if branch is None:
            # condition 必須兩個分支都有（WF-E003），走到這裡代表
            # 發布後 DSL 被改過，或驗證有漏。
            raise ApplicationError(
                f"節點 {node['id']} 缺少 when={result} 的分支",
                type="MissingBranch",
                non_retryable=True,
            )
        return branch

    async def _run_human(self, node: dict[str, Any]) -> "_Outcome":
        node_id = node["id"]

        # 1. 解析參與者。查資料庫，因此是 Activity。
        spec = node.get("resolver") or node.get("assignee") or {}
        raw = await workflow.execute_activity(
            "resolve_participants",
            args=[
                {
                    "tenant_id": self._input.tenant_id,
                    "instance_id": self._input.instance_id,
                    "spec": spec,
                    "business_object": self._business,
                }
            ],
            start_to_close_timeout=ACTIVITY_TIMEOUT,
            result_type=list[Participant],
        )
        participants = [
            p if isinstance(p, Participant) else Participant(**p) for p in raw
        ]

        if not participants:
            # 靜默卡住是最糟的結果：流程看似在跑，實際永遠沒人能處理。
            raise ApplicationError(
                f"節點 {node_id} 找不到任何參與者，流程無法繼續",
                type="NoParticipants",
                non_retryable=True,
            )

        # 2. 建立待辦
        task_ids = await workflow.execute_activity(
            "create_human_tasks",
            args=[
                {
                    "tenant_id": self._input.tenant_id,
                    "instance_id": self._input.instance_id,
                    "node_id": node_id,
                    "node_label": node.get("label"),
                    "participants": [p.__dict__ for p in participants],
                    "participant_kind": node.get("participant", "internal"),
                    "form_key": node.get("form_key"),
                    "due_at": self._deadline_iso(node),
                }
            ],
            start_to_close_timeout=ACTIVITY_TIMEOUT,
            result_type=list[str],
        )

        # 這個節點自己的等待狀態。並行時每個節點各一份，
        # 決策才不會落到別的分支。
        wait = _NodeWait(
            participants=participants,
            pending=list(task_ids),
            deadline=self._deadline(node),
        )
        self._waits[node_id] = wait

        try:
            verdict = await self._await_decisions(node, wait)
        finally:
            # 無論正常完成、退回或例外，都要清掉未處理的待辦，
            # 否則使用者的收件匣留下永遠點不動的項目。
            await self._cancel_pending(node_id, "節點已完成")
            self._waits.pop(node_id, None)

        return self._route_after_human(node, verdict)

    async def _await_decisions(
        self, node: dict[str, Any], wait: "_NodeWait"
    ) -> Verdict:
        policy = self._join_policy(node)

        def decided() -> Verdict:
            # 分母取當下的參與者數。ESCALATE 加簽後會變大——
            # 加簽者與原簽核人地位相同，要一起算。
            return evaluate_policy(
                wait.results,
                len(wait.participants),
                policy,
                now=workflow.now(),
                deadline=wait.deadline,
            )

        # 提醒的時間點。由晚到早排序後逐一消化。
        pending_reminders = self._reminder_points(node, wait.deadline)

        node_id = node["id"]

        def woken() -> bool:
            """該醒來了：有人簽了，或被要求快轉"""
            return decided() is not Verdict.WAIT or node_id in self._skip_time

        while True:
            verdict = decided()
            if verdict is not Verdict.WAIT:
                return verdict

            # 快轉：直接走逾時處理，不假造任何決策。
            # ESCALATE/WAIT 不離開節點（deadline 被清掉），
            # 其餘交給呼叫端路由——與真的等到逾時完全同一條路。
            if node_id in self._skip_time:
                self._skip_time.discard(node_id)
                handled = await self._handle_timeout_inline(node, wait)
                if not handled:
                    return Verdict.TIMEOUT
                pending_reminders = []
                continue

            if wait.deadline is None:
                await workflow.wait_condition(woken)
                continue

            # 下一個要醒來的時間點：提醒或到期，看哪個先到
            next_at = wait.deadline
            is_reminder = False
            if pending_reminders:
                at, _ = pending_reminders[0]
                if at < next_at:
                    next_at = at
                    is_reminder = True

            remaining = next_at - workflow.now()
            if remaining.total_seconds() > 0:
                try:
                    await workflow.wait_condition(woken, timeout=remaining)
                    # 有人簽了或被要求快轉，下一輪由迴圈頭判斷
                    continue
                except asyncio.TimeoutError:
                    pass

            if is_reminder:
                _, label = pending_reminders.pop(0)
                await self._send_reminder(node, wait, label)
                continue

            # 逾時。ESCALATE 與 WAIT 不離開節點，其餘交給呼叫端路由。
            handled = await self._handle_timeout_inline(node, wait)
            if not handled:
                return Verdict.TIMEOUT
            # ESCALATE/WAIT 之後 deadline 被清掉，剩下的提醒沒有意義
            pending_reminders = []

    def _reminder_points(
        self, node: dict[str, Any], deadline: Any
    ) -> list[tuple[Any, str]]:
        """算出提醒的絕對時間點

        remind_at 的每個值是「距離到期還有多久」，不是「從現在起算」。
        P2D 代表到期前兩天，因此絕對時間是 deadline - P2D。

        回傳 (時間點, 原始標籤) 依時間由早到晚排序。
        標籤要留著——提醒訊息要告訴使用者「還剩 P1D」，
        而不是一個算出來的時間戳。
        """
        if deadline is None:
            return []

        raw = (node.get("timeout") or {}).get("remind_at") or []
        points: list[tuple[Any, str]] = []

        for label in raw:
            delta = parse_iso_duration(label)
            if delta is None:
                workflow.logger.warning(
                    "節點 %s 的 remind_at 值無法解析，忽略：%s", node["id"], label
                )
                continue

            at = deadline - delta
            # 比現在還早的時間點已經過去了——例如期限一天卻設
            # 「到期前三天提醒」，那個點在流程開始之前。
            if at <= workflow.now():
                continue
            points.append((at, label))

        points.sort(key=lambda p: p[0])
        return points

    async def _send_reminder(
        self, node: dict[str, Any], wait: "_NodeWait", remaining: str
    ) -> None:
        """發出逾時前提醒

        發給這個節點的簽核人，不是申請人——要催的是還沒簽的那些人。

        失敗不中斷流程：寄信失敗而讓整張單卡住，業務上說不通。
        這與 notification 節點的處理一致。
        """
        done = {r.participant_id for r in wait.results}
        targets = [p.id for p in wait.participants if p.id not in done]

        if not targets:
            return

        try:
            await workflow.execute_activity(
                "send_notification",
                args=[
                    {
                        "tenant_id": self._input.tenant_id,
                        "instance_id": self._input.instance_id,
                        "node_id": node["id"],
                        "kind": "reminder",
                        "channel": node.get("remind_channel") or ["email", "line"],
                        "to": targets,
                        "template_key": "task_reminder",
                        "remaining": remaining,
                        "business_object": self._business,
                    }
                ],
                start_to_close_timeout=ACTIVITY_TIMEOUT,
                retry_policy=TemporalRetry(maximum_attempts=3),
            )
        except Exception as e:  # noqa: BLE001
            workflow.logger.warning(
                "節點 %s 的提醒發送失敗，流程繼續：%s", node["id"], e
            )

    async def _handle_timeout_inline(
        self, node: dict[str, Any], wait: "_NodeWait"
    ) -> bool:
        """在節點內處理逾時

        回傳 True 代表已處理、繼續等待；False 代表要離開節點
        （由 _route_timeout 決定去向）。

        ESCALATE 與 WAIT 都屬於「繼續等」：前者加簽後等，後者單純等。
        AUTO_APPROVE 與 AUTO_REJECT 則要離開節點。
        """
        policy = self._timeout_policy(node)

        if policy is TimeoutPolicy.WAIT:
            # 語意就是繼續等。清掉 deadline 改為無限等待，
            # 否則 wait_condition 會立刻再次逾時，形成忙迴圈。
            wait.deadline = None
            return True

        if policy is TimeoutPolicy.ESCALATE:
            await self._escalate(node, wait)
            return True

        return False

    async def _escalate(self, node: dict[str, Any], wait: "_NodeWait") -> None:
        """加簽給指定的人

        語意（2026-09-17 定案）：
          - 原本的待辦**保留**，兩邊都能簽。是「加簽」不是「換人」。
          - 加簽者**算進**分母，與原簽核人地位相同。

        加簽只做一次：清掉 deadline 讓後續無限等待。
        否則每次逾時都再加一個人，最後所有人都收到待辦。
        """
        spec = (node.get("timeout") or {}).get("to")
        if not spec:
            raise ApplicationError(
                f"節點 {node['id']} 的 timeout policy 是 ESCALATE 但缺少 to",
                type="MissingEscalateTarget",
                non_retryable=True,
            )

        raw = await workflow.execute_activity(
            "resolve_participants",
            args=[
                {
                    "tenant_id": self._input.tenant_id,
                    "instance_id": self._input.instance_id,
                    "spec": spec,
                    "business_object": self._business,
                }
            ],
            start_to_close_timeout=ACTIVITY_TIMEOUT,
            result_type=list[Participant],
        )
        extra = [p if isinstance(p, Participant) else Participant(**p) for p in raw]

        # 排除已經在名單裡的人，避免同一人收到兩張待辦
        known = {p.id for p in wait.participants}
        extra = [p for p in extra if p.id not in known]

        if not extra:
            # 解析不到加簽對象（N3，2026-09-19 定案）
            #
            # 原本只留一行 WARNING，然後繼續等。問題不在「繼續等」
            # ——原簽核人本來就還簽得動，讓流程失敗反而會把一張
            # 跑到一半的單弄死。問題在於**沒有任何人看得到**：
            # 畫面與稽核都不會出現這件事。
            #
            # 因此照舊繼續等，但把異常掛到實例上讓監控頁紅字標出。
            label = node.get("label") or node["id"]
            await workflow.execute_activity(
                "raise_attention",
                args=[
                    {
                        "tenant_id": self._input.tenant_id,
                        "instance_id": self._input.instance_id,
                        "code": "ESCALATE_UNRESOLVED",
                        "detail": (
                            f"節點「{label}」逾時加簽，但解析不到新的簽核人。"
                            "流程仍在等原簽核人處理。"
                        ),
                    }
                ],
                start_to_close_timeout=ACTIVITY_TIMEOUT,
            )
            wait.deadline = None
            return

        task_ids = await workflow.execute_activity(
            "create_human_tasks",
            args=[
                {
                    "tenant_id": self._input.tenant_id,
                    "instance_id": self._input.instance_id,
                    "node_id": node["id"],
                    "node_label": f"{node.get('label') or node['id']}（逾時加簽）",
                    "participants": [p.__dict__ for p in extra],
                    "participant_kind": node.get("participant", "internal"),
                    "form_key": node.get("form_key"),
                    "due_at": None,
                }
            ],
            start_to_close_timeout=ACTIVITY_TIMEOUT,
            result_type=list[str],
        )

        wait.participants = [*wait.participants, *extra]
        wait.pending = [*wait.pending, *task_ids]
        wait.deadline = None

    def _timeout_policy(self, node: dict[str, Any]) -> TimeoutPolicy:
        raw = (node.get("timeout") or {}).get("policy", "WAIT")
        try:
            return TimeoutPolicy(raw)
        except ValueError:
            return TimeoutPolicy.WAIT

    def _route_after_human(self, node: dict[str, Any], verdict: Verdict) -> "_Outcome":
        if verdict is Verdict.CONTINUE:
            return _Outcome(next_node=self._next_of(node["id"]))

        if verdict is Verdict.TIMEOUT:
            return self._route_timeout(node)

        # REJECT
        return self._route_reject(node, node.get("on_reject"))

    def _route_timeout(self, node: dict[str, Any]) -> "_Outcome":
        """逾時後的去向

        只會收到 AUTO_APPROVE 與 AUTO_REJECT——WAIT 與 ESCALATE
        在 _handle_timeout_inline 就處理掉了，不會離開節點。
        """
        policy = self._timeout_policy(node)

        if policy is TimeoutPolicy.AUTO_APPROVE:
            return _Outcome(next_node=self._next_of(node["id"]))

        if policy is TimeoutPolicy.AUTO_REJECT:
            return self._route_reject(node, node.get("on_reject"))

        # 理論上到不了。真的到了代表 _handle_timeout_inline 漏了分支，
        # 明確失敗比靜默走退回好——後者會讓使用者以為是有人退回的。
        raise ApplicationError(
            f"節點 {node['id']} 的逾時策略 {policy.value} 未被處理",
            type="UnhandledTimeoutPolicy",
            non_retryable=True,
        )

    def _route_reject(
        self, node: dict[str, Any], on_reject: dict[str, Any] | None
    ) -> "_Outcome":
        spec = on_reject or {"action": "end", "result": "rejected"}

        if spec.get("action") == "goto":
            target = spec.get("node")
            if target and target in self._nodes:
                return _Outcome(next_node=target)
            raise ApplicationError(
                f"節點 {node['id']} 的退回目標 {target} 不存在",
                type="InvalidRejectTarget",
                non_retryable=True,
            )

        result = spec.get("result", "rejected")
        status = (
            InstanceStatus.CANCELLED if result == "cancelled" else InstanceStatus.REJECTED
        )
        return _Outcome(
            finished=InstanceResult(
                status=status,
                path=self._path,
                output={"business_object": self._business},
            )
        )

    async def _run_action(self, node: dict[str, Any]) -> "_Outcome":
        retry = node.get("retry") or {}
        attempts = int(retry.get("max_attempts", 3))

        try:
            output = await workflow.execute_activity(
                "run_action",
                args=[
                    {
                        "tenant_id": self._input.tenant_id,
                        "instance_id": self._input.instance_id,
                        "node_id": node["id"],
                        "action": node.get("action"),
                        "input_mapping": node.get("input_mapping") or {},
                        "business_object": self._business,
                        # 冪等鍵讓重試不會產生兩張訂單。
                        # 展開後的值必須在重試間保持一致，因此用
                        # instance_id 與 node_id 而非時間戳。
                        "idempotency_key": self._render_key(node),
                    }
                ],
                start_to_close_timeout=timedelta(minutes=5),
                retry_policy=TemporalRetry(maximum_attempts=attempts),
                result_type=dict,
            )
        except Exception as e:  # noqa: BLE001 - 需區分是否有 on_failure
            on_failure = node.get("on_failure")
            if on_failure:
                return self._route_reject(node, on_failure)
            raise ApplicationError(
                f"節點 {node['id']} 執行失敗：{e}",
                type="ActionFailed",
            ) from e

        # Activity 可回傳要合併回業務物件的欄位，例如 Odoo 訂單號
        if isinstance(output, dict):
            self._business.update(output.get("business_object") or {})

        return _Outcome(next_node=self._next_of(node["id"]))

    async def _run_notification(self, node: dict[str, Any]) -> "_Outcome":
        # 通知失敗不該中斷流程。單據已經核准，卻因為寄信失敗
        # 而讓整張單卡住，業務上說不通。
        try:
            await workflow.execute_activity(
                "send_notification",
                args=[
                    {
                        "tenant_id": self._input.tenant_id,
                        "instance_id": self._input.instance_id,
                        "node_id": node["id"],
                        "channel": node.get("channel") or ["email"],
                        "to": node.get("to") or [],
                        "template_key": node.get("template_key"),
                        "business_object": self._business,
                    }
                ],
                start_to_close_timeout=ACTIVITY_TIMEOUT,
                retry_policy=TemporalRetry(maximum_attempts=3),
            )
        except Exception as e:  # noqa: BLE001
            workflow.logger.warning("節點 %s 通知失敗，流程繼續：%s", node["id"], e)

        return _Outcome(next_node=self._next_of(node["id"]))

    # ── 並行 ────────────────────────────────────────────

    async def _run_parallel(self, node: dict[str, Any]) -> "_Outcome":
        """並行分支與匯合

        用 asyncio.gather 讓分支同時跑，Temporal 會持久化每個分支的
        進度——重啟、當機、重放都由它保證。不另外建 parallel_group 表：
        兩份狀態就有兩份要對帳。

        分支是完整的子流程，可以有多個節點、條件判斷、巢狀的並行。
        每個分支走到 join 就停下，由這裡統一判定。
        """
        node_id = node["id"]
        branch_starts = self._branches_of(node_id)

        if not branch_starts:
            raise ApplicationError(
                f"節點 {node_id} 是 parallel 但沒有任何分支",
                type="NoBranches",
                non_retryable=True,
            )

        join_id = self._find_join(branch_starts)
        if join_id is None:
            raise ApplicationError(
                f"節點 {node_id} 的分支沒有匯合到 join 節點",
                type="NoJoinNode",
                non_retryable=True,
            )

        join_node = self._nodes[join_id]
        policy = self._join_policy(join_node)
        total = len(branch_starts)

        # 每個分支跑成獨立的 asyncio task，這樣才能在
        # ANY / N_OF_M / ANY_REJECT 滿足時取消其餘分支。
        tasks = [
            asyncio.ensure_future(self._run_branch(start, join_id, name))
            for name, start in enumerate(branch_starts)
        ]

        outcomes: list[_BranchOutcome] = []

        def verdict() -> Verdict:
            # 把分支結果轉成 policy engine 看得懂的形狀。
            # 一個分支 = 一個「參與者」，分支的核准/退回 = 該參與者的決策。
            # 這樣並行與單節點多人簽核共用同一套判定邏輯，語意一致。
            return evaluate_policy(
                [o.as_task_result() for o in outcomes],
                total,
                policy,
                now=workflow.now(),
                deadline=None,
            )

        # join 的逾時。並簽時某人長期不簽會讓整個流程卡住，
        # 沒有這個出口就只能人工介入。
        deadline = self._deadline(join_node)
        timed_out = False

        pending = set(tasks)
        while pending:
            if deadline is not None:
                remaining = (deadline - workflow.now()).total_seconds()
                if remaining <= 0:
                    timed_out = True
                    break
                done, pending = await workflow.wait(
                    pending,
                    return_when=asyncio.FIRST_COMPLETED,
                    timeout=remaining,
                )
                if not done:
                    # 逾時而非有分支完成
                    timed_out = True
                    break
            else:
                done, pending = await workflow.wait(
                    pending, return_when=asyncio.FIRST_COMPLETED
                )

            for t in done:
                exc = t.exception()
                if exc is not None:
                    # 分支內的錯誤不該被吞掉。取消其餘分支後往上拋，
                    # 由 run() 的統一處理寫成 FAILED。
                    for other in pending:
                        other.cancel()
                    raise exc
                outcomes.append(t.result())

            v = verdict()
            if v is not Verdict.WAIT:
                # 條件已滿足，不必等其餘分支。
                # 三人會簽第一人否決時，另外兩人不該白跑。
                for other in pending:
                    other.cancel()
                if pending:
                    await self._await_cancelled(pending)
                break

        if timed_out:
            outcome = await self._on_join_timeout(node, join_node, pending, outcomes)
            if outcome is not None:
                return outcome
            # 回到這裡代表 ESCALATE/WAIT 已處理，繼續等剩下的分支
            for t in pending:
                try:
                    outcomes.append(await t)
                except Exception:  # noqa: BLE001
                    raise

        self._path.append(join_id)

        v = verdict()
        if v is Verdict.CONTINUE:
            return _Outcome(next_node=self._next_of(join_id))

        # 退回。join 節點自己可以有 on_reject，沒有的話用 parallel 的。
        return self._route_reject(
            join_node, join_node.get("on_reject") or node.get("on_reject")
        )

    async def _on_join_timeout(
        self,
        parallel_node: dict[str, Any],
        join_node: dict[str, Any],
        pending: set,
        outcomes: list["_BranchOutcome"],
    ) -> "_Outcome | None":
        """join 逾時的處置

        回傳 _Outcome 代表要離開並行；None 代表繼續等剩下的分支。

        AUTO_APPROVE / AUTO_REJECT 會取消未完成的分支——那些分支的
        待辦由分支自己的 finally 清掉。
        """
        policy = self._timeout_policy(join_node)
        join_id = join_node["id"]

        if policy is TimeoutPolicy.WAIT:
            # 語意就是繼續等
            return None

        if policy is TimeoutPolicy.ESCALATE:
            # join 層級的加簽沒有明確語意：分支各有自己的簽核人，
            # 要加給誰並不清楚。明確失敗而非猜測。
            raise ApplicationError(
                f"join 節點 {join_id} 不支援 ESCALATE，"
                "請改在各分支的簽核節點設定",
                type="UnsupportedJoinTimeout",
                non_retryable=True,
            )

        # AUTO_APPROVE / AUTO_REJECT：取消未完成的分支
        for t in pending:
            t.cancel()
        if pending:
            await self._await_cancelled(pending)

        self._path.append(join_id)

        if policy is TimeoutPolicy.AUTO_APPROVE:
            return _Outcome(next_node=self._next_of(join_id))

        return self._route_reject(
            join_node, join_node.get("on_reject") or parallel_node.get("on_reject")
        )

    async def _run_branch(
        self, start: str, join_id: str, name: int
    ) -> "_BranchOutcome":
        """跑一個分支到 join 為止"""
        result = await self._walk(start, stop_at=join_id)

        if result.status is InstanceStatus.RUNNING:
            # 正常抵達 join
            return _BranchOutcome(name=name, approved=True)

        if result.status in (InstanceStatus.REJECTED, InstanceStatus.CANCELLED):
            return _BranchOutcome(name=name, approved=False)

        # FAILED 或分支自己走到了 end —— 兩者都是 DSL 設計問題。
        # 分支不該有自己的出口，它必須匯合。
        raise ApplicationError(
            result.error or f"分支 {start} 未抵達匯合點就結束了",
            type="BranchDidNotJoin",
            non_retryable=True,
        )

    async def _await_cancelled(self, tasks: set) -> None:
        """等被取消的分支收尾

        取消只是送出請求，分支要跑完自己的 finally（取消待辦）才算結束。
        不等的話流程可能在待辦還沒取消時就往下走。
        """
        for t in tasks:
            try:
                await t
            except (asyncio.CancelledError, Exception):  # noqa: BLE001
                # 取消造成的例外是預期的，分支的清理在它自己的 finally
                pass

    def _branches_of(self, node_id: str) -> list[str]:
        """parallel 節點的所有出邊目標"""
        return [e[1] for e in self._edges if e[0] == node_id]

    def _find_join(self, branch_starts: list[str]) -> str | None:
        """找分支匯合的 join 節點

        從各分支起點往下走，第一個型別為 join 的節點就是。
        分支可能有多個節點，因此要走訪而非只看直接後繼。
        """
        for start in branch_starts:
            seen: set[str] = set()
            queue = [start]
            while queue:
                current = queue.pop(0)
                if current in seen:
                    continue
                seen.add(current)

                node = self._nodes.get(current)
                if node is None:
                    continue
                if node.get("type") == "join":
                    return current

                queue.extend(e[1] for e in self._edges if e[0] == current)
        return None

    def _finish(self, node: dict[str, Any]) -> InstanceResult:
        result = node.get("result", "completed")
        mapping = {
            "completed": InstanceStatus.COMPLETED,
            "rejected": InstanceStatus.REJECTED,
            "cancelled": InstanceStatus.CANCELLED,
        }
        return InstanceResult(
            status=mapping.get(result, InstanceStatus.COMPLETED),
            path=self._path,
            output={"business_object": self._business},
        )

    # ── Signal 與 Query ─────────────────────────────────

    @workflow.signal(name="task_completed")
    async def task_completed(self, payload: TaskCompleted) -> None:
        if isinstance(payload, dict):
            payload = TaskCompleted(**payload)

        # 決策要落到對應的節點。並行時多個節點同時等待，
        # 送錯節點會讓別的分支誤以為自己完成了。
        wait = self._waits.get(payload.node_id)
        if wait is None:
            workflow.logger.info(
                "忽略非活躍節點的決策：task=%s node=%s active=%s",
                payload.task_id,
                payload.node_id,
                list(self._waits),
            )
            return

        # 重複 Signal 不重複計算。前端重試或網路重送都可能發生。
        if any(r.task_id == payload.task_id for r in wait.results):
            return

        if payload.task_id not in wait.pending:
            workflow.logger.info("忽略不在待辦清單中的決策：%s", payload.task_id)
            return

        wait.results.append(
            TaskResult(
                task_id=payload.task_id,
                participant_id=payload.participant_id,
                decision=payload.decision,
                comment=payload.comment,
                decided_at=workflow.now().isoformat(),
            )
        )

    @workflow.signal(name="skip_time")
    async def skip_time(self, node_id: str = "") -> None:
        """時間快轉（只限沙箱）

        把某個節點的等待視為已到期，讓真正的逾時處理立刻執行。
        含 P2D、P7D 的流程在模擬時原本要等兩天、七天才看得到
        逾時行為，等於無法驗證。

        **不假造決策**：跑的是 `_handle_timeout_inline`，
        與真的等到逾時走同一條路。假造一個 APPROVE 會讓模擬
        顯示「通過了」，而正式環境的 AUTO_REJECT 其實是退回——
        那比不給快轉更糟。

        正式租戶一律拒絕。授權在 HTTP 端已經擋過一層，
        這裡再擋一層：Signal 是能繞過 HTTP 的介面，
        而快轉會讓單據在沒人簽的情況下前進。
        """
        if self._input is None or not self._input.is_sandbox:
            workflow.logger.warning(
                "拒絕非沙箱實例的時間快轉：tenant=%s node=%s",
                self._input.tenant_id if self._input else "?",
                node_id,
            )
            return

        if node_id and node_id not in self._waits:
            workflow.logger.info(
                "忽略非活躍節點的快轉：node=%s active=%s", node_id, list(self._waits)
            )
            return

        # 沒指定節點就快轉全部等待中的節點（平行分支時方便）
        self._skip_time |= {node_id} if node_id else set(self._waits)

    @workflow.signal(name="cancel_instance")
    async def cancel_instance(self, reason: str = "") -> None:
        self._cancelled = True
        self._cancel_reason = reason or "使用者取消"
        # 實際中斷由 Temporal 的 cancel 機制處理。這個 Signal 讓
        # 呼叫端可以附上原因，而 RequestCancel 沒有帶原因的欄位。

    @workflow.query(name="current_state")
    def current_state(self) -> CurrentState:
        active = list(self._waits)
        pending: list[str] = []
        results: list[dict[str, Any]] = []
        for wait in self._waits.values():
            pending.extend(wait.pending)
            results.extend(r.__dict__ for r in wait.results)

        return CurrentState(
            # 並行時有多個活躍節點，current_node 取第一個。
            # 精確的位置請看 active_nodes。
            current_node=active[0] if active else None,
            path=self._path,
            pending_task_ids=pending,
            results=results,
            business_object=self._business,
            active_nodes=active,
        )

    # ── 輔助 ────────────────────────────────────────────

    def _find_start(self) -> str | None:
        for node_id, node in self._nodes.items():
            if node.get("type") == "trigger":
                return node_id
        return None

    def _next_of(self, node_id: str, when: bool | None = None) -> str | None:
        """沿 edges 找後繼節點

        when 為 None 時取第一條沒有 when 標記的邊；
        指定時取 when 相符的邊。
        """
        for edge in self._edges:
            if edge[0] != node_id:
                continue
            meta = edge[2] if len(edge) > 2 and isinstance(edge[2], dict) else {}
            edge_when = meta.get("when")

            if when is None:
                if edge_when is None:
                    return edge[1]
            elif edge_when is when:
                return edge[1]
        return None

    def _join_policy(self, node: dict[str, Any]) -> JoinPolicy:
        raw = node.get("join")
        if isinstance(raw, dict):
            return JoinPolicy(**raw)
        # 未指定時單人簽核即可前進。多人並簽一定會寫明 join，
        # 沒寫代表設計者預期只有一個人。
        return JoinPolicy()

    def _deadline(self, node: dict[str, Any]):
        spec = node.get("timeout") or {}
        after = spec.get("after")
        if not after:
            return None
        delta = parse_iso_duration(after)
        if delta is None:
            return None
        # 以流程進入此節點的時間為起點。用 workflow.now() 而非
        # 系統時鐘，重放時才會得到同樣的截止時間。
        return workflow.now() + delta

    def _deadline_iso(self, node: dict[str, Any]) -> str | None:
        d = self._deadline(node)
        return d.isoformat() if d else None

    def _render_key(self, node: dict[str, Any]) -> str:
        template = node.get("idempotency_key") or ""
        if not template:
            return f"{self._input.instance_id}:{node['id']}"

        # 只支援 instance.id 與 node.id 兩個變數。更複雜的模板
        # 需要求值引擎，而冪等鍵只要穩定且唯一即可。
        return (
            template.replace("{{instance.id}}", str(self._input.instance_id))
            .replace("{{node.id}}", str(node["id"]))
        )

    def _context(self, node_id: str) -> dict[str, Any]:
        """條件求值用的資料根

        node_id 由呼叫端傳入而非讀取「當前節點」：並行時同時有多個
        節點在執行，沒有單一的當前節點可言。表達式裡的 node.id
        必須指向正在求值的那一個。
        """
        ctx = dict(self._business)
        ctx["instance"] = {"id": self._input.instance_id}
        ctx["node"] = {"id": node_id}
        return ctx

    async def _cancel_pending(self, node_id: str | None, reason: str) -> None:
        """取消未處理的待辦

        node_id 為 None 時取消全部（流程被取消或失敗時），
        指定時只取消該節點的（單一節點完成時）。
        """
        if node_id is None:
            waits = list(self._waits.values())
            self._waits.clear()
        else:
            wait = self._waits.get(node_id)
            waits = [wait] if wait is not None else []

        to_cancel: list[str] = []
        for wait in waits:
            done = {r.task_id for r in wait.results}
            to_cancel.extend(t for t in wait.pending if t not in done)
            wait.pending = []

        if not to_cancel:
            return

        try:
            await workflow.execute_activity(
                "cancel_human_tasks",
                args=[
                    {
                        "tenant_id": self._input.tenant_id,
                        "task_ids": to_cancel,
                        "reason": reason,
                    }
                ],
                start_to_close_timeout=ACTIVITY_TIMEOUT,
            )
        except Exception as e:  # noqa: BLE001
            # 取消失敗只會留下孤兒待辦，可由對帳程序清理。
            # 讓整個流程因此失敗反而更糟。
            workflow.logger.warning("取消待辦失敗：%s", e)


class _NodeWait:
    """一個簽核節點的等待狀態

    並行時每個活躍節點各一份。共用一份的話，分支之間的決策會互相
    污染——財務的核准被法務節點看見，policy engine 立刻誤判為完成。
    """

    __slots__ = ("participants", "pending", "results", "deadline")

    def __init__(
        self,
        participants: list[Participant],
        pending: list[str],
        deadline: Any = None,
    ) -> None:
        self.participants = participants
        self.pending = pending
        self.results: list[TaskResult] = []
        self.deadline = deadline


class _BranchOutcome:
    """一個並行分支的結果

    轉成 TaskResult 交給 policy engine，讓並行與單節點多人簽核
    共用同一套判定邏輯：一個分支等同一個「參與者」。
    """

    __slots__ = ("name", "approved")

    def __init__(self, name: int, approved: bool) -> None:
        self.name = name
        self.approved = approved

    def as_task_result(self) -> TaskResult:
        return TaskResult(
            task_id=f"branch-{self.name}",
            participant_id=f"branch-{self.name}",
            decision=Decision.APPROVE if self.approved else Decision.REJECT,
        )


class _Outcome:
    """節點執行的結果：往下一個節點，或流程結束"""

    __slots__ = ("next_node", "finished")

    def __init__(
        self,
        next_node: str | None = None,
        finished: InstanceResult | None = None,
    ) -> None:
        self.next_node = next_node
        self.finished = finished
