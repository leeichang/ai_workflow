-- 開發模式（沙箱）
--
-- 為何用影子租戶而不是同租戶加標記：
--   RLS 已經在租戶層做好隔離（九項測試通過），tenant_tx() 是所有
--   DB 存取的單一進入點。建一個沙箱租戶，隔離就自動成立，
--   不需要在任何查詢加 where is_sandbox = false。
--
--   反過來用「同租戶加 is_sandbox 標記」的話，每一處查詢都要記得過濾，
--   漏一個就洩漏——而且是靜默失敗。靜默的錯誤比會爆炸的錯誤危險。
--
-- 沙箱不是另一個環境，是一個附身模式：測試者始終是同一個真實使用者，
-- 只是在該次簽核操作上標註自己扮演誰（見 audit_event.acting_as）。

alter table tenant
  add column parent_tenant_id uuid references tenant(id),
  add column is_sandbox boolean not null default false,
  add column expires_at timestamptz;

comment on column tenant.parent_tenant_id is
    '沙箱租戶的來源租戶。正式租戶為 NULL';
comment on column tenant.is_sandbox is
    '沙箱租戶。前端據此顯示不可關閉的警示標示，清理程序據此回收。
     簽核 API 的授權開洞也綁在這個欄位上——沙箱身分不是一種權限，
     是一種環境狀態，所以不可做成角色';
comment on column tenant.expires_at is
    '沙箱租戶的到期時間。正式租戶為 NULL，永不過期。
     已有 574 個測試租戶的前例，不設期限會無限膨脹';

-- 沙箱一定有來源，正式租戶一定沒有。用約束擋住中間狀態，
-- 否則「is_sandbox 但沒有 parent」這種資料會讓清理程序不知道往哪回收。
alter table tenant
  add constraint tenant_sandbox_has_parent check (
      (is_sandbox and parent_tenant_id is not null and expires_at is not null)
      or (not is_sandbox and parent_tenant_id is null)
  );

create index if not exists tenant_sandbox_idx
    on tenant (parent_tenant_id) where is_sandbox;

-- 一次開發模式的紀錄
--
-- 與 tenant 分開的理由：租戶被回收之後，「某人在某時建過沙箱」
-- 這件事要留著。租戶是資源，session 是行為紀錄。
create table sandbox_session (
    id                uuid primary key default uuid_generate_v4(),
    -- 來源租戶。沙箱租戶被回收後這筆仍指得到來源
    tenant_id         uuid not null references tenant(id) on delete cascade,
    -- 沙箱租戶。回收時設為 NULL，保留這筆紀錄
    sandbox_tenant_id uuid references tenant(id) on delete set null,
    change_request_id uuid,
    -- 測試者。這是模擬簽核授權開洞的第二個條件：
    -- 必須是沙箱的建立者本人，不是「有某個角色的人」
    created_by        uuid not null references app_user(id),
    status            text not null default 'ACTIVE'
                      check (status in ('ACTIVE', 'RESET', 'EXPIRED', 'DISCARDED')),
    created_at        timestamptz not null default now(),
    expires_at        timestamptz not null
);

comment on table sandbox_session is
    '一次開發模式的紀錄。沙箱租戶回收後仍保留，
     因為「某人在某時建過沙箱」是稽核需要的資訊';

-- 以沙箱租戶反查 session 是授權檢查的必經路徑（tasks.rs 的開洞條件），
-- 每次模擬簽核都會查，要有索引
create index if not exists sandbox_session_sandbox_idx
    on sandbox_session (sandbox_tenant_id) where status = 'ACTIVE';

create index if not exists sandbox_session_owner_idx
    on sandbox_session (tenant_id, created_by, status);

-- 沙箱的稽核要能分辨「誰按的」與「扮演誰」
--
-- 只記 acting_as 的話，稽核會顯示張文華核准了某張單，而他根本不知道
-- 有這回事。沙箱裡雖然無害，但兩套稽核語意會讓程式碼分岔——
-- 寧可從一開始就一致。
alter table audit_event
  add column acting_as uuid;

comment on column audit_event.acting_as is
    '模擬簽核時被扮演的對象。actor_id 仍是真正按下按鈕的人。
     正式操作為 NULL';
