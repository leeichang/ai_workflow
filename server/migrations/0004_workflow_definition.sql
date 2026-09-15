-- 流程定義與版本
--
-- 結構與 form_definition 刻意保持一致：同樣的草稿與版本模型、
-- 同樣的 immutable 保護、同樣的 RLS policy。
-- 兩者在使用者心智中都是「設計後發布的定義」，行為一致可降低學習成本。
--
-- 為何流程與表單分開而非合併：
--   一張表單可被多個流程使用（報價單流程、緊急報價流程），
--   一個流程也可能不綁表單（排程觸發的資料同步）。
--   合併會讓兩者的版本綁死，改流程被迫升表單版本。

create table workflow_definition (
    id              uuid primary key default uuid_generate_v4(),
    tenant_id       uuid not null references tenant(id) on delete cascade,
    workflow_key    text not null,
    business_object text not null,
    name            text not null,
    description     text,
    created_by      uuid references app_user(id) on delete set null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    unique (tenant_id, workflow_key)
);

create index workflow_definition_tenant_idx on workflow_definition (tenant_id);
create index workflow_definition_bo_idx on workflow_definition (tenant_id, business_object);

comment on table workflow_definition is
    '流程的身份。實際內容在 workflow_definition_version';

create table workflow_definition_version (
    id           uuid primary key default uuid_generate_v4(),
    tenant_id    uuid not null references tenant(id) on delete cascade,
    workflow_id  uuid not null references workflow_definition(id) on delete cascade,

    -- null 表示草稿。每個流程最多一份草稿。
    version      integer,

    -- 完整的 DSL，符合 schemas/workflow-dsl.schema.json
    content      jsonb not null,

    status       text not null default 'DRAFT'
                 check (status in ('DRAFT', 'PUBLISHED', 'ARCHIVED')),

    created_by   uuid references app_user(id) on delete set null,
    created_at   timestamptz not null default now(),
    updated_at   timestamptz not null default now(),
    published_by uuid references app_user(id) on delete set null,
    published_at timestamptz,

    constraint wf_published_has_version check (
        status <> 'PUBLISHED' or (version is not null and published_at is not null)
    ),
    constraint wf_draft_has_no_version check (
        status <> 'DRAFT' or version is null
    )
);

create index workflow_version_tenant_idx on workflow_definition_version (tenant_id);
create index workflow_version_wf_idx
    on workflow_definition_version (workflow_id, version desc nulls first);

create unique index workflow_version_single_draft
    on workflow_definition_version (workflow_id)
    where status = 'DRAFT';

create unique index workflow_version_unique
    on workflow_definition_version (workflow_id, version)
    where version is not null;

-- DSL 內容查詢會用到，例如找出所有引用某個角色的流程
create index workflow_version_content_idx
    on workflow_definition_version using gin (content);

comment on column workflow_definition_version.content is
    '符合 schemas/workflow-dsl.schema.json。發布時由後端做兩層驗證：
     JSON Schema 驗結構，圖結構驗語意（WF-E001~E008）';

-- 已發布版本不可修改。與 form_definition_version 共用同一個 trigger 函式。
create trigger workflow_version_immutable
    before update on workflow_definition_version
    for each row execute function forbid_published_mutation();

create trigger workflow_version_touch
    before update on workflow_definition_version
    for each row execute function touch_updated_at();

create trigger workflow_definition_touch
    before update on workflow_definition
    for each row execute function touch_updated_at();

-- ── RLS ─────────────────────────────────────────────────
do $$
declare t text;
begin
    foreach t in array array['workflow_definition', 'workflow_definition_version']
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
