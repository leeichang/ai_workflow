-- 流程實例與人工待辦
--
-- Temporal 存流程的「執行狀態」，這裡存的是業務視角的投影。
-- 兩者職責不同，不是重複：
--   Temporal  事件歷史、重放、排程。retention 過期後清除。
--   這裡      查詢、報表、稽核。永久保存。
--
-- 收件匣「我有哪些待辦」是高頻查詢，走 Temporal 的 Visibility API
-- 會慢且受 retention 限制，因此待辦一律落在本地資料庫。
--
-- workflow_id 的組成見 server/crates/temporal-client/src/workflow_id.rs：
--   {tenant_id}:{business_object}:{instance_id}
-- 前綴是租戶隔離的一部分，不只是命名慣例。

create table workflow_instance (
    id              uuid primary key default uuid_generate_v4(),
    tenant_id       uuid not null references tenant(id) on delete cascade,

    -- 啟動時使用的流程版本。流程定義可能之後又發布新版，
    -- 但進行中的實例必須沿用啟動當下的那一版，否則簽核途中規則會變。
    workflow_version_id uuid not null
        references workflow_definition_version(id) on delete restrict,

    business_object text not null,
    -- 業務單號，例如 QT-2026-0001。與 tenant 組成 workflow_id。
    business_key    text not null,

    -- Temporal 的 workflow_id 與 run_id。
    -- run_id 會因 continue-as-new 改變，僅供追查用，不可拿來定位流程。
    temporal_workflow_id text not null,
    temporal_run_id      text,

    status text not null default 'RUNNING'
           check (status in ('RUNNING', 'COMPLETED', 'REJECTED', 'CANCELLED', 'FAILED')),

    -- 啟動時的業務資料快照，供稽核比對
    input        jsonb not null default '{}'::jsonb,
    -- 流程結束時的輸出
    output       jsonb,
    error        text,

    started_by   uuid references app_user(id) on delete set null,
    started_at   timestamptz not null default now(),
    ended_at     timestamptz,
    updated_at   timestamptz not null default now(),

    -- 同一個業務單號同時只能有一個進行中的流程。
    -- 少了這個約束，重複點擊送出會產生兩張簽核單。
    unique (tenant_id, temporal_workflow_id)
);

create index workflow_instance_tenant_idx on workflow_instance (tenant_id);
create index workflow_instance_status_idx
    on workflow_instance (tenant_id, status, started_at desc);
create index workflow_instance_bo_idx
    on workflow_instance (tenant_id, business_object, business_key);

comment on table workflow_instance is
    '流程執行的業務投影。真相在 Temporal，但查詢與稽核走這裡';
comment on column workflow_instance.temporal_run_id is
    'continue-as-new 後會改變，不可用於定位流程。定位一律用 temporal_workflow_id';

create table human_task (
    id          uuid primary key default uuid_generate_v4(),
    tenant_id   uuid not null references tenant(id) on delete cascade,
    instance_id uuid not null references workflow_instance(id) on delete cascade,

    -- DSL 中的節點 id
    node_id     text not null,
    node_label  text,

    -- 指派對象。角色與使用者擇一，兩者都有時以使用者優先。
    assignee_user_id uuid references app_user(id) on delete set null,
    assignee_role    text,

    -- internal 為員工，external 為客戶。external 走 Portal，權限不同。
    participant_kind text not null default 'internal'
                     check (participant_kind in ('internal', 'external')),

    form_key    text,

    status text not null default 'PENDING'
           check (status in ('PENDING', 'APPROVED', 'REJECTED', 'CANCELLED', 'EXPIRED')),

    decision    text check (decision in ('APPROVE', 'REJECT')),
    comment     text,

    due_at      timestamptz,
    created_at  timestamptz not null default now(),
    decided_at  timestamptz,
    decided_by  uuid references app_user(id) on delete set null,
    updated_at  timestamptz not null default now()
);

-- 收件匣查詢：某人的待辦。這是全系統最高頻的查詢。
create index human_task_inbox_idx
    on human_task (tenant_id, assignee_user_id, status, created_at desc)
    where status = 'PENDING';

create index human_task_role_inbox_idx
    on human_task (tenant_id, assignee_role, status, created_at desc)
    where status = 'PENDING';

create index human_task_instance_idx on human_task (instance_id, node_id);

-- 逾期掃描
create index human_task_due_idx
    on human_task (tenant_id, due_at)
    where status = 'PENDING' and due_at is not null;

comment on table human_task is
    '人工待辦。由 Workflow 的 Activity 建立，使用者決策後送 Signal 回 Temporal';
comment on column human_task.decision is
    '僅 human_approval 節點有值。human_task 節點只有完成與否，沒有核准語意';

create trigger workflow_instance_touch
    before update on workflow_instance
    for each row execute function touch_updated_at();

create trigger human_task_touch
    before update on human_task
    for each row execute function touch_updated_at();

-- ── RLS ─────────────────────────────────────────────────
do $$
declare t text;
begin
    foreach t in array array['workflow_instance', 'human_task']
    loop
        execute format('alter table %I enable row level security', t);
        execute format('alter table %I force row level security', t);
        execute format($p$
            create policy tenant_isolation on %I
            using (tenant_id = current_setting('app.tenant_id', true)::uuid)
            with check (tenant_id = current_setting('app.tenant_id', true)::uuid)
        $p$, t);
    end loop;
end $$;
