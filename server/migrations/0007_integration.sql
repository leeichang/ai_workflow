-- 組織資料同步
--
-- 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §5、§10。
--
-- 本次實作 P1（FILE connection：Excel / CSV 匯入），但表結構一次做對——
-- §5 的主張是四種接入方式共用同一條管線，差別只在最前面的 Extractor。
-- 為 FILE 另建一套表，等 P2 的 REST 進來時就要搬資料。
--
-- 刻意不建的表見 §10.2：integration_dataset（只有兩個 dataset，
-- 寫死在程式碼即可）、integration_schema（canonical schema 由
-- schemas/ 下的 JSON Schema 管）、integration_connection_version。
--
-- §10.1 列的 integration_raw_record（Raw Layer）也沒建，理由與 §10.2
-- 列的那些不同，所以寫在這裡：
--   - 它的用途是「保留原始列供比對與重跑」。P1 是 Excel 上傳，
--     原始檔案本身就是 raw layer，客戶手上有同一份
--   - 存原始列等於把整份員工個資再複製一份進資料庫。多一份個資
--     就多一個外洩面，而 P1 拿不到相對的好處
--   - P2 的 REST 進來時值得重新評估：API 回應不像檔案那樣留得住，
--     那時 Raw Layer 才真的有「可重跑」的價值

-- ── 連線 ────────────────────────────────────────────────

create table integration_connection (
    id            uuid primary key default uuid_generate_v4(),
    tenant_id     uuid not null references tenant(id) on delete cascade,
    name          text not null,
    kind          text not null
                  check (kind in ('FILE', 'REST', 'SQL')),
    -- 非機密的連線設定。例如 FILE 的編碼、REST 的 base_url
    config        jsonb not null default '{}',
    -- 憑證的參照，指向 Secret Manager。**不存明文**（WDC §8）
    credential_ref text,
    enabled       boolean not null default true,
    created_by    uuid references app_user(id),
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),

    unique (tenant_id, name)
);

comment on table integration_connection is
    '怎麼連。FILE 不需要憑證，REST / SQL 的帳密一律存 credential_ref
     指向 Secret Manager，不進 config——config 會出現在 API 回應與日誌裡';

comment on column integration_connection.credential_ref is
    '憑證參照。絕不存明文帳密（需求 §14 的第一條安全檢查項）';

create index connection_tenant_idx on integration_connection (tenant_id);

-- ── 來源 ────────────────────────────────────────────────
--
-- connection × dataset。同一條連線可以同時供應員工與部門兩份資料。

create table integration_source (
    id            uuid primary key default uuid_generate_v4(),
    tenant_id     uuid not null references tenant(id) on delete cascade,
    connection_id uuid not null references integration_connection(id)
                  on delete cascade,
    -- employee 或 department。寫死在程式碼，不另建 dataset 表（§10.2）
    dataset       text not null
                  check (dataset in ('employee', 'department')),
    name          text not null,
    -- 抓什麼。FILE 用不到（檔案就是全部），SQL 放具名查詢的識別碼
    query_definition   jsonb not null default '{}',
    -- 來源欄位 → canonical 欄位。§8 的 AI 建議確認後固化在這裡
    mapping_definition jsonb not null default '{}',
    -- 完整性閘的門檻（§7 規則 2）。低於上次成功筆數 × 此值即中止
    completeness_threshold numeric(4,3) not null default 0.900
        check (completeness_threshold > 0 and completeness_threshold <= 1),
    -- 上次成功同步的筆數。完整性閘的比較基準
    last_success_count integer,
    last_synced_at     timestamptz,
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),

    unique (tenant_id, connection_id, dataset)
);

comment on column integration_source.mapping_definition is
    '來源欄位名 → canonical 欄位名。一經確認即固化，後續同步不再問 AI——
     每次重問會讓結果漂移（§8）';

comment on column integration_source.completeness_threshold is
    '完整性閘門檻。來源 1000 人只回 823 人時，若直接刪 177 人且其中
     有主管，全公司相關流程立刻卡死（§7）';

comment on column integration_source.last_success_count is
    'NULL 代表從未成功同步過。首次匯入沒有比較基準，完整性閘不生效';

create index source_tenant_idx on integration_source (tenant_id);
create index source_connection_idx on integration_source (connection_id);

-- ── 同步批次 ────────────────────────────────────────────

create table integration_sync_run (
    id          uuid primary key default uuid_generate_v4(),
    tenant_id   uuid not null references tenant(id) on delete cascade,
    source_id   uuid not null references integration_source(id)
                on delete cascade,
    -- MANUAL（Sync Now）或 SCHEDULE。§6 不做 webhook 與 on-demand
    trigger     text not null default 'MANUAL'
                check (trigger in ('MANUAL', 'SCHEDULE')),
    status      text not null default 'RUNNING'
                check (status in ('RUNNING', 'SUCCESS', 'PARTIAL_SUCCESS',
                                  'ABORTED_INCOMPLETE', 'FAILED')),
    -- 試跑。preview 不寫入任何資料，但仍留下一筆 run 供比對
    dry_run     boolean not null default false,

    -- §10.3 的計數契約。每一個都要有——
    -- 同步不能只回「success」，管理員要看得出兩邊差異有多大
    source_count    integer,
    received_count  integer not null default 0,
    parsed_count    integer not null default 0,
    mapped_count    integer not null default 0,
    validated_count integer not null default 0,
    inserted_count  integer not null default 0,
    updated_count   integer not null default 0,
    unchanged_count integer not null default 0,
    skipped_by_ownership_count integer not null default 0,
    rejected_count  integer not null default 0,
    missing_in_source_count    integer not null default 0,

    -- 中止或失敗的原因。給管理員看，不是給程式判斷用
    message     text,
    started_at  timestamptz not null default now(),
    finished_at timestamptz,
    started_by  uuid references app_user(id)
);

comment on table integration_sync_run is
    '同步批次與完整計數。日誌不得寫入 API password / token、身分證號、
     完整 email 清單（§10.3、WDC §30）';

comment on column integration_sync_run.source_count is
    '來源宣稱的總筆數。FILE 沒有這個概念（檔案有幾列就是幾列），
     REST 的分頁回應通常有 total';

comment on column integration_sync_run.skipped_by_ownership_count is
    '因 platform_managed_fields 而跳過的**欄位**數，不是筆數。
     管理員據此知道平台與 ERP 的差異有多大（§3 規則 4）';

create index sync_run_tenant_idx on integration_sync_run (tenant_id);
create index sync_run_source_idx
    on integration_sync_run (source_id, started_at desc);

-- ── 單筆問題明細 ────────────────────────────────────────

create table integration_sync_issue (
    id          uuid primary key default uuid_generate_v4(),
    tenant_id   uuid not null references tenant(id) on delete cascade,
    run_id      uuid not null references integration_sync_run(id)
                on delete cascade,
    -- 來源的第幾列。讓管理員回到 Excel 找得到那一行
    row_number  integer,
    kind        text not null
                check (kind in ('AMBIGUOUS', 'VALIDATION_FAILED',
                                'SKIPPED_BY_OWNERSHIP', 'MISSING_IN_SOURCE')),
    message     text not null,
    -- 出問題的欄位。SKIPPED_BY_OWNERSHIP 一定有值
    field       text,
    -- 匹配到的既有資料。AMBIGUOUS 時會有多筆
    matched_ids uuid[] not null default '{}',
    created_at  timestamptz not null default now()
);

comment on table integration_sync_issue is
    '單筆層級的問題明細。不存原始值——那會讓這張表變成另一份個資副本';

comment on column integration_sync_issue.kind is
    'AMBIGUOUS 匹配到多筆（不猜，見 §5.2）；VALIDATION_FAILED 驗證擋下；
     SKIPPED_BY_OWNERSHIP 因平台維護而跳過；MISSING_IN_SOURCE 來源消失';

create index sync_issue_tenant_idx on integration_sync_issue (tenant_id);
create index sync_issue_run_idx on integration_sync_issue (run_id);

-- ── 待停用清單 ──────────────────────────────────────────
--
-- §7 規則 3：MISSING_IN_SOURCE 連續 N 次仍消失才進這張清單，
-- 由管理員確認後才停用。不自動停用——那等於平台替客戶做人事決定。

create table integration_pending_deactivation (
    id           uuid primary key default uuid_generate_v4(),
    tenant_id    uuid not null references tenant(id) on delete cascade,
    source_id    uuid not null references integration_source(id)
                 on delete cascade,
    -- 目前只有 employee 會進待停用。部門消失的處理見 §7
    user_id      uuid not null references app_user(id) on delete cascade,
    -- 連續幾次在來源查不到
    miss_count   integer not null default 1,
    first_missed_at timestamptz not null default now(),
    last_missed_at  timestamptz not null default now(),
    -- 停用前置檢查（§7 規則 4）的結果。有阻擋原因時不可停用
    blocked_reason  text,
    created_at   timestamptz not null default now(),

    unique (tenant_id, source_id, user_id)
);

comment on column integration_pending_deactivation.blocked_reason is
    '此人是他人的 manager_id、是某部門的 manager_user_id、或有未完成的
     Human Task 時，擋下停用並要求先轉派（§7 規則 4）';

create index pending_deactivation_tenant_idx
    on integration_pending_deactivation (tenant_id);

-- ── RLS ─────────────────────────────────────────────────
--
-- 與既有表一致：FORCE 且 fail-closed。
-- integration_connection 的 credential_ref 尤其不可跨租戶讀到。

alter table integration_connection enable row level security;
alter table integration_connection force row level security;
create policy connection_tenant_isolation on integration_connection
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

alter table integration_source enable row level security;
alter table integration_source force row level security;
create policy source_tenant_isolation on integration_source
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

alter table integration_sync_run enable row level security;
alter table integration_sync_run force row level security;
create policy sync_run_tenant_isolation on integration_sync_run
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

alter table integration_sync_issue enable row level security;
alter table integration_sync_issue force row level security;
create policy sync_issue_tenant_isolation on integration_sync_issue
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

alter table integration_pending_deactivation enable row level security;
alter table integration_pending_deactivation force row level security;
create policy pending_deactivation_tenant_isolation
    on integration_pending_deactivation
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

grant select, insert, update, delete on integration_connection to workflow_app;
grant select, insert, update, delete on integration_source to workflow_app;
grant select, insert, update, delete on integration_sync_run to workflow_app;
grant select, insert, update, delete on integration_sync_issue to workflow_app;
grant select, insert, update, delete
    on integration_pending_deactivation to workflow_app;
