-- 流程監控角色與異常標記
--
-- 對應 N1（誰看得到全部流程）與 N3（解析不到加簽對象時的行為），
-- 兩者都在 2026-09-19 由使用者定案。
--
-- ── N1：process_monitor ─────────────────────────────────
-- 原本只有 admin 看得到全部流程。把 approver/finance_manager
-- 一併放行是錯的方向——那些角色的語意是「會簽到某些單」而非
-- 「該看到全部單」，混用之後很難再拆開。
--
-- 因此另立一個角色，語意單一：營運端負責盯流程、排除卡關的人。
--
-- ── N3：needs_attention ────────────────────────────────
-- 逾時加簽解析不到對象時，原本只留一行 WARNING，畫面與稽核都
-- 看不到。改成在實例上掛旗標：流程照跑（原簽核人仍能簽），
-- 但監控頁紅字標出「需要人介入」。
--
-- 刻意不做成 status 的一個值：status 是流程生命週期
-- （RUNNING/COMPLETED/...），異常是正交的維度。塞進 status
-- 會讓「異常但仍在跑」無法表達，而那正是這裡要的語意。

-- ── 角色 ────────────────────────────────────────────────
-- 對每個既有租戶補上。新租戶由建租戶的流程負責，
-- 但目前還沒有那條流程，所以這裡對 tenant 全表掃。
insert into role (tenant_id, code, name, is_system)
select t.id, 'process_monitor', '流程監控員', true
from tenant t
on conflict (tenant_id, code) do nothing;

-- ── 異常標記 ────────────────────────────────────────────
alter table workflow_instance
    add column needs_attention boolean not null default false,
    -- 機器可讀的原因代碼。前端據此決定顯示哪一種說明與處理建議，
    -- 不靠比對中文訊息——訊息會改寫，代碼不會。
    add column attention_code text,
    -- 給人看的一句話。包含節點名稱這類當下才知道的資訊，
    -- 無法由 code 反推，因此兩者都存。
    add column attention_detail text,
    add column attention_at timestamptz;

-- 監控頁的預設排序是「異常的排前面」，而異常在總量裡是少數。
-- 部分索引只收異常的列，比全表索引小一個數量級。
create index workflow_instance_attention_idx
    on workflow_instance (tenant_id, attention_at desc)
    where needs_attention;

comment on column workflow_instance.needs_attention is
    '需要人介入。流程仍在跑，不是失敗狀態';
comment on column workflow_instance.attention_code is
    'ESCALATE_UNRESOLVED 等。前端據此分支，不比對訊息文字';
