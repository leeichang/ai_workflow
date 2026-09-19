-- 組織基本資料擴充
--
-- 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md 的 P0。
--
-- 本次只做「資料模型」，不做同步機制（connection / mapping / sync run）。
-- 理由：手動建得起組織、表單綁得到人與部門，客戶才有東西可 demo；
-- 同步是把資料灌進來的管道，管道要等容器先在。
--
-- 三個核心設計：
--
--   §2 人員與登入帳號分離
--      同步進來的人多數不會登入（產線作業員、不用電腦的主管），
--      但他們必須存在——可能是別人的 manager_id、可能是部門主管、
--      可能要被指派待辦。password_hash 因此轉 nullable。
--
--   §3 欄位級 ownership
--      D-14「ERP 優先、平台補缺」要能成立，同步時必須知道哪些欄位
--      不准碰。沒有 platform_managed_fields，管理員補的資料會在
--      下一次同步被沖掉。
--
--   §7 不硬刪
--      來源系統查不到的人不刪除，改標記 sync_status。
--      硬刪會讓進行中流程的 assignee 變成孤兒。

-- ── 部門擴充 ────────────────────────────────────────────

alter table department
    add column external_id              text,
    add column source_system            text not null default 'MANUAL'
        check (source_system in ('MANUAL', 'ODOO', 'SQL', 'FILE')),
    add column status                   text not null default 'ACTIVE'
        check (status in ('ACTIVE', 'INACTIVE')),
    add column sort_order               integer not null default 0,
    add column platform_managed_fields  text[] not null default '{}',
    add column last_synced_at           timestamptz;

comment on column department.external_id is
    '來源系統的主鍵。匹配的第一順位——external_id 比 code 可靠，
     因為客戶可能改部門代碼但不會改 ERP 的內部 id';

comment on column department.source_system is
    'MANUAL 表示平台手動建立，不參與同步。同步寫入時據此判斷歸屬';

comment on column department.status is
    '來源系統查不到時標記 INACTIVE 而非刪除。
     硬刪會讓 department.manager_user_id 的參照與進行中流程失效';

comment on column department.platform_managed_fields is
    '平台維護的欄位名清單。同步時這些欄位一律跳過。
     典型情境：ERP 沒維護部門主管，管理員在平台補上，
     下次同步不該把它清掉';

-- external_id 在同一來源系統內唯一。MANUAL 建立的沒有 external_id，
-- 用 partial index 排除 null（多筆 null 不算衝突）
create unique index department_external_idx
    on department (tenant_id, source_system, external_id)
    where external_id is not null;

-- ── 使用者擴充 ──────────────────────────────────────────

alter table app_user
    add column employee_no             text,
    add column job_title               text,
    add column hired_at                date,
    add column left_at                 date,
    add column phone                   text,
    add column extension               text,
    add column external_id             text,
    add column source_system           text not null default 'MANUAL'
        check (source_system in ('MANUAL', 'ODOO', 'SQL', 'FILE')),
    add column can_login               boolean not null default true,
    add column platform_managed_fields text[] not null default '{}',
    add column sync_status             text not null default 'PLATFORM_ONLY'
        check (sync_status in ('SYNCED', 'PLATFORM_ONLY', 'MISSING_IN_SOURCE')),
    add column last_synced_at          timestamptz;

comment on column app_user.employee_no is
    '員工編號。匹配的第二順位（第一順位是 external_id）';

comment on column app_user.left_at is
    '離職日。決定簽核可用性——已離職者不該被解析為簽核人。
     保留資料列而非刪除，因為歷史簽核記錄要查得到人';

comment on column app_user.can_login is
    '能否登入平台。同步進來的產線人員通常為 false——
     他們要存在（可能是別人的主管、可能被指派待辦）但不需要帳號';

comment on column app_user.sync_status is
    'SYNCED 來源系統有此人；PLATFORM_ONLY 平台自建（不參與同步）；
     MISSING_IN_SOURCE 曾同步過但來源已查不到。
     最後一種不自動停用——連續幾次才進待停用清單由應用層決定';

create unique index app_user_employee_no_idx
    on app_user (tenant_id, employee_no)
    where employee_no is not null;

create unique index app_user_external_idx
    on app_user (tenant_id, source_system, external_id)
    where external_id is not null;

create index app_user_can_login_idx
    on app_user (tenant_id, can_login)
    where can_login = true;

-- ── password_hash 轉 nullable ───────────────────────────
--
-- 這是既有表的破壞性變更（需求 07 的 Q-01，2026-09-18 定案採此方案）。
--
-- 安全性要求：登入流程必須顯式檢查 can_login = true
-- and password_hash is not null，不可依賴「有 hash 就能登入」的
-- 隱含假設。約束在此處補上，應用層同樣要檢查（雙重保險）。

alter table app_user alter column password_hash drop not null;

-- 可登入的帳號一定要有密碼。這條約束讓「can_login = true 但沒有
-- password_hash」這種不一致狀態進不了資料庫。
alter table app_user add constraint app_user_login_needs_password
    check (not can_login or password_hash is not null);

comment on column app_user.password_hash is
    '可為 NULL——無登入權限的人員（can_login = false）不需要密碼。
     登入流程必須同時檢查 can_login 與此欄位';

-- ── 代理人 ──────────────────────────────────────────────
--
-- 平台自有資料，不同步。ERP 不會有「簽核代理」這種概念。

create table approval_delegation (
    id            uuid primary key default uuid_generate_v4(),
    tenant_id     uuid not null references tenant(id) on delete cascade,
    delegator_id  uuid not null references app_user(id) on delete cascade,
    delegate_id   uuid not null references app_user(id) on delete cascade,
    starts_at     date not null,
    ends_at       date not null,
    reason        text,
    created_by    uuid not null references app_user(id),
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),

    -- 代理給自己沒有意義，而且展開時會無限迴圈
    constraint delegation_not_self check (delegator_id <> delegate_id),
    constraint delegation_date_order check (ends_at >= starts_at)
);

comment on table approval_delegation is
    '簽核代理。平台自有資料，不參與同步——ERP 沒有這個概念。
     展開語意（取代還是雙方都收到）見需求 07 的 Q-05，尚未定案';

create index delegation_tenant_idx on approval_delegation (tenant_id);
create index delegation_delegator_idx
    on approval_delegation (delegator_id, starts_at, ends_at);

-- RLS。與既有表一致：FORCE 且 fail-closed
alter table approval_delegation enable row level security;
alter table approval_delegation force row level security;

create policy delegation_tenant_isolation on approval_delegation
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

grant select, insert, update, delete on approval_delegation to workflow_app;

-- ── employee view ───────────────────────────────────────
--
-- 對外（EIG canonical schema、未來的 MCP tool、AI 欄位對應）
-- 一律用 employee 這個名字。資料庫實體是 app_user，用 view 對齊。
--
-- 需求 07 §2 選擇擴充 app_user 而非新建 employee 表的理由：
-- Approver Resolver、user_role、Human Task、通知、RLS 全部零改動。

create view employee as
select
    id            as employee_id,
    tenant_id,
    employee_no,
    name,
    email,
    department_id,
    manager_id,
    job_title,
    hired_at,
    left_at,
    phone,
    extension,
    status,
    can_login,
    sync_status
from app_user;

comment on view employee is
    'app_user 的對外名稱。EIG canonical dataset 用 employee 這個詞，
     資料庫實體沿用既有的 app_user 以避免改動 Resolver 與 Human Task';

grant select on employee to workflow_app;
