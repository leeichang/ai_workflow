-- 稽核事件
--
-- append-only。不可 UPDATE、不可 DELETE，由 trigger 與權限雙重保證。
-- 這是合規要求，也是出事時唯一可信的紀錄。

create table audit_event (
    id          bigserial primary key,
    tenant_id   uuid not null references tenant(id) on delete restrict,

    -- 誰做的
    actor_kind  text not null check (actor_kind in ('internal', 'external', 'system')),
    actor_id    text,              -- internal 為 user_id，external 為 contact_id，system 為 null
    actor_name  text,              -- 當下的顯示名，供日後人員異動後仍可追溯

    -- 做了什麼
    action      text not null,     -- 例如 form.publish、task.approve
    target_type text not null,     -- 例如 form_definition_version
    target_id   text,

    -- 細節
    payload     jsonb not null default '{}',

    -- 從哪來
    ip          inet,
    user_agent  text,
    request_id  text,              -- 串連同一請求的多筆事件

    at          timestamptz not null default now()
);

create index audit_tenant_at_idx on audit_event (tenant_id, at desc);
create index audit_target_idx on audit_event (tenant_id, target_type, target_id);
create index audit_actor_idx on audit_event (tenant_id, actor_id) where actor_id is not null;
create index audit_action_idx on audit_event (tenant_id, action);
create index audit_payload_idx on audit_event using gin (payload);

comment on table audit_event is
    'append-only。UPDATE 與 DELETE 由 trigger 阻擋，正式環境另以 DB 角色權限限制';
comment on column audit_event.actor_name is
    '記錄當下的顯示名。人員離職刪除後，稽核紀錄仍可讀';

create or replace function forbid_audit_mutation() returns trigger
language plpgsql as $$
begin
    raise exception '稽核事件不可% （id=%）',
        case tg_op when 'UPDATE' then '修改' else '刪除' end,
        coalesce(old.id::text, '?')
        using errcode = 'restrict_violation';
end $$;

create trigger audit_no_update
    before update on audit_event
    for each row execute function forbid_audit_mutation();

create trigger audit_no_delete
    before delete on audit_event
    for each row execute function forbid_audit_mutation();

alter table audit_event enable row level security;
alter table audit_event force row level security;

-- 讀取受租戶隔離；寫入也檢查，避免寫錯租戶
create policy tenant_isolation on audit_event
    using (tenant_id = current_setting('app.tenant_id', true)::uuid)
    with check (tenant_id = current_setting('app.tenant_id', true)::uuid);

-- ── 權限層防護 ──────────────────────────────────────────
-- trigger 已阻擋 UPDATE 與 DELETE，此處再從權限層收回，
-- 即使日後有人誤刪 trigger 也不會失守。
do $$
begin
    if exists (select 1 from pg_roles where rolname = 'workflow_app') then
        revoke update, delete on audit_event from workflow_app;
        grant select, insert on audit_event to workflow_app;
        grant usage, select on sequence audit_event_id_seq to workflow_app;
    end if;
end $$;
