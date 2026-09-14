-- 租戶、使用者、部門、角色
--
-- 租戶隔離策略（架構鐵律一）：
--   所有業務表帶 tenant_id 並啟用 RLS。
--   連線中介層每個請求執行 SET LOCAL app.tenant_id，
--   policy 檢查 tenant_id = current_setting('app.tenant_id')。
--   未設定時查詢回空，不是回全部，這是刻意的 fail-closed 設計。

create extension if not exists "uuid-ossp";

-- ── 租戶 ────────────────────────────────────────────────
-- 唯一不帶 tenant_id 的表，因為它自己就是租戶。
create table tenant (
    id          uuid primary key default uuid_generate_v4(),
    code        text not null unique,
    name        text not null,
    timezone    text not null default 'Asia/Taipei',
    currency    text not null default 'TWD',
    locale      text not null default 'zh-TW',
    logo_key    text,
    status      text not null default 'ACTIVE'
                check (status in ('ACTIVE', 'SUSPENDED')),
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

comment on table tenant is '租戶。不帶 tenant_id，不啟用 RLS，僅平台管理者可存取';

-- ── 部門 ────────────────────────────────────────────────
create table department (
    id              uuid primary key default uuid_generate_v4(),
    tenant_id       uuid not null references tenant(id) on delete cascade,
    code            text not null,
    name            text not null,
    parent_id       uuid references department(id) on delete set null,
    manager_user_id uuid,   -- 循環參照，於 0002 補 FK
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    unique (tenant_id, code)
);

create index department_tenant_idx on department (tenant_id);
create index department_parent_idx on department (parent_id) where parent_id is not null;

-- ── 使用者 ──────────────────────────────────────────────
create table app_user (
    id            uuid primary key default uuid_generate_v4(),
    tenant_id     uuid not null references tenant(id) on delete cascade,
    email         text not null,
    name          text not null,
    password_hash text not null,
    department_id uuid references department(id) on delete set null,
    manager_id    uuid references app_user(id) on delete set null,
    status        text not null default 'ACTIVE'
                  check (status in ('ACTIVE', 'DISABLED')),
    last_login_at timestamptz,
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),
    unique (tenant_id, email)
);

create index app_user_tenant_idx on app_user (tenant_id);
create index app_user_dept_idx on app_user (department_id) where department_id is not null;
create index app_user_manager_idx on app_user (manager_id) where manager_id is not null;

comment on column app_user.manager_id is
    '直屬主管。Approver Resolver 的 manager_of 型別依此解析，執行時即時查詢不快照';

alter table department
    add constraint department_manager_fk
    foreign key (manager_user_id) references app_user(id) on delete set null;

-- ── 角色 ────────────────────────────────────────────────
-- MVP 固定五種，不開放自訂（決議：功能規劃 1.2）
create table role (
    id          uuid primary key default uuid_generate_v4(),
    tenant_id   uuid not null references tenant(id) on delete cascade,
    code        text not null,
    name        text not null,
    is_system   boolean not null default false,
    created_at  timestamptz not null default now(),
    unique (tenant_id, code)
);

create index role_tenant_idx on role (tenant_id);

create table user_role (
    user_id     uuid not null references app_user(id) on delete cascade,
    role_id     uuid not null references role(id) on delete cascade,
    tenant_id   uuid not null references tenant(id) on delete cascade,
    granted_at  timestamptz not null default now(),
    granted_by  uuid references app_user(id) on delete set null,
    primary key (user_id, role_id)
);

create index user_role_tenant_idx on user_role (tenant_id);
create index user_role_role_idx on user_role (role_id);

-- ── RLS ─────────────────────────────────────────────────
-- 每張帶 tenant_id 的表都套用相同 policy。
-- current_setting 第二參數 true 表示未設定時回 null 而非報錯，
-- null 與任何值比較皆為 false，查詢自然回空。

do $$
declare t text;
begin
    foreach t in array array['department', 'app_user', 'role', 'user_role']
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

comment on table app_user is
    'RLS 已啟用且 FORCE。即使是 table owner 也受限，避免遷移腳本意外跨租戶';

-- ── updated_at 自動更新 ─────────────────────────────────
create or replace function touch_updated_at() returns trigger
language plpgsql as $$
begin
    new.updated_at := now();
    return new;
end $$;

create trigger tenant_touch     before update on tenant     for each row execute function touch_updated_at();
create trigger department_touch before update on department for each row execute function touch_updated_at();
create trigger app_user_touch   before update on app_user   for each row execute function touch_updated_at();
