-- 資料庫角色
--
-- 為何需要這份 migration：
--   PostgreSQL 的 superuser 會完全繞過 Row Level Security，
--   連 FORCE ROW LEVEL SECURITY 都擋不住。
--   若應用程式用 superuser 連線，租戶隔離形同虛設。
--
-- 角色分工：
--   app          容器建立的 superuser。只用於執行 migration 與維運。
--   workflow_app 應用程式連線用。非 superuser，受 RLS 約束。
--
-- 此檔必須最先執行。

do $$
begin
    if not exists (select 1 from pg_roles where rolname = 'workflow_app') then
        -- 密碼由環境變數注入，此處為開發預設值
        create role workflow_app login password 'app_dev_only'
            nosuperuser nocreatedb nocreaterole noinherit;
    end if;
end $$;

comment on role workflow_app is
    '應用程式連線角色。非 superuser，受 RLS 約束。正式環境須改密碼';

-- 連線與 schema 使用權
grant connect on database workflow to workflow_app;
grant usage on schema public to workflow_app;

-- 現有與未來建立的表、序列，一律授予基本 DML
-- 注意：不給 DDL 權限，migration 由 app 角色執行
grant select, insert, update, delete on all tables in schema public to workflow_app;
grant usage, select on all sequences in schema public to workflow_app;

alter default privileges in schema public
    grant select, insert, update, delete on tables to workflow_app;
alter default privileges in schema public
    grant usage, select on sequences to workflow_app;

-- 稽核表的額外權限限制在 0003_audit.sql 末尾處理，
-- 因為該表此時尚未建立。
