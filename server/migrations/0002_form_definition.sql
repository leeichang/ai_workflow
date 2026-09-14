-- 表單定義與版本
--
-- 版本策略（架構鐵律：定義與執行分離）：
--   發布後 immutable。要改就升版，不動已發布的內容。
--   執行中的流程實例綁定啟動時的版本，不受後續發布影響。
--
-- 為何草稿與已發布版本同表：
--   草稿是 version = null 的特殊列，發布時分配版本號。
--   分兩張表會讓「取當前可編輯內容」需要 union，且容易漏同步。

create table form_definition (
    id              uuid primary key default uuid_generate_v4(),
    tenant_id       uuid not null references tenant(id) on delete cascade,
    form_key        text not null,
    business_object text not null,
    name            text not null,
    created_by      uuid references app_user(id) on delete set null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    unique (tenant_id, form_key)
);

create index form_definition_tenant_idx on form_definition (tenant_id);

comment on table form_definition is
    '表單的身份。實際內容在 form_definition_version';

create table form_definition_version (
    id              uuid primary key default uuid_generate_v4(),
    tenant_id       uuid not null references tenant(id) on delete cascade,
    form_id         uuid not null references form_definition(id) on delete cascade,

    -- null 表示草稿。每個 form 最多一份草稿，由部分唯一索引保證。
    version         integer,

    -- 完整的表單定義，符合 schemas/form.schema.json
    content         jsonb not null,

    status          text not null default 'DRAFT'
                    check (status in ('DRAFT', 'PUBLISHED', 'ARCHIVED')),

    created_by      uuid references app_user(id) on delete set null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    published_by    uuid references app_user(id) on delete set null,
    published_at    timestamptz,

    -- 已發布版本必須有版本號與發布時間
    constraint published_has_version check (
        status <> 'PUBLISHED' or (version is not null and published_at is not null)
    ),
    -- 草稿不得有版本號
    constraint draft_has_no_version check (
        status <> 'DRAFT' or version is null
    )
);

create index form_version_tenant_idx on form_definition_version (tenant_id);
create index form_version_form_idx on form_definition_version (form_id, version desc nulls first);

-- 每個表單最多一份草稿
create unique index form_version_single_draft
    on form_definition_version (form_id)
    where status = 'DRAFT';

-- 版本號在表單內唯一
create unique index form_version_unique
    on form_definition_version (form_id, version)
    where version is not null;

-- content 內的欄位查詢會用到，例如找出所有綁定某資料路徑的表單
create index form_version_content_idx on form_definition_version using gin (content);

comment on column form_definition_version.content is
    '符合 schemas/form.schema.json。發布時由後端驗證，不信任前端';
comment on column form_definition_version.version is
    'null 為草稿。發布時取 max(version) + 1';

-- ── 發布保護 ────────────────────────────────────────────
-- 已發布版本不得修改內容。這是資料層的最後防線，
-- 應用層也會擋，但雙重保險可防止遷移腳本或手動操作出錯。

create or replace function forbid_published_mutation() returns trigger
language plpgsql as $$
begin
    if old.status = 'PUBLISHED' then
        -- 只允許改為 ARCHIVED，其餘欄位不得動
        if new.content is distinct from old.content
           or new.version is distinct from old.version
           or new.published_at is distinct from old.published_at then
            raise exception '已發布的版本不可修改（form_version_id=%）', old.id
                using errcode = 'restrict_violation';
        end if;
    end if;
    return new;
end $$;

create trigger form_version_immutable
    before update on form_definition_version
    for each row execute function forbid_published_mutation();

create trigger form_version_touch
    before update on form_definition_version
    for each row execute function touch_updated_at();

create trigger form_definition_touch
    before update on form_definition
    for each row execute function touch_updated_at();

-- ── RLS ─────────────────────────────────────────────────
do $$
declare t text;
begin
    foreach t in array array['form_definition', 'form_definition_version']
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
