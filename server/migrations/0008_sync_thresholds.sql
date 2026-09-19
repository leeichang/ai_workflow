-- 同步的兩個門檻參數
--
-- 對應需求 07 的 Q-02 與 Q-03，兩者在 2026-09-19 定案。

-- ── Q-02：完整性閘改成「比例且絕對值」雙條件 ────────────
--
-- 只看比例的話，50 人的公司走掉 6 人（12%）就誤觸發 0.9 的門檻。
-- 那不是資料缺漏，是正常的人員異動——管理員被擋下之後只能調高門檻，
-- 而調高之後保護就變弱了。
--
-- 改成兩個條件都成立才中止：比例低於 threshold **且** 減少超過
-- min_drop 人。小公司的正常異動不誤擋，大公司的大量消失仍擋得住。

alter table integration_source
    add column min_drop_threshold integer not null default 10
        check (min_drop_threshold >= 1);

comment on column integration_source.min_drop_threshold is
    '完整性閘的絕對值條件。減少人數低於此值時不中止，即使比例不過。
     預設 10——50 人的公司走掉 6 人是正常異動，不該被擋（Q-02）';

-- ── Q-03：待停用的連續消失次數 ──────────────────────────
--
-- 預設 3 次。搭配每日同步約 3 天，是「離職者仍可能收到任務」的
-- 窗口長度。
--
-- 做成每個來源可調而非寫死：不同客戶的匯出頻率不同。
-- 每週匯出一次的客戶，3 次就是三週。

alter table integration_source
    add column deactivation_miss_threshold integer not null default 3
        check (deactivation_miss_threshold >= 1);

comment on column integration_source.deactivation_miss_threshold is
    '連續消失幾次才進入待停用清單。預設 3，搭配每日同步約 3 天。
     這是離職者仍能收到任務的窗口長度（Q-03）';

-- ── 待停用的確認狀態 ────────────────────────────────────
--
-- §7 規則 3：由管理員確認後才停用。**不自動停用**——
-- 那等於平台替客戶做人事決定。
--
-- 管理員也可能判斷「這個人其實還在，只是匯出漏了」，
-- 那要能忽略而不是被迫停用。

alter table integration_pending_deactivation
    add column status text not null default 'PENDING'
        check (status in ('PENDING', 'CONFIRMED', 'DISMISSED')),
    add column resolved_at timestamptz,
    add column resolved_by uuid references app_user(id);

comment on column integration_pending_deactivation.status is
    'PENDING 待管理員決定；CONFIRMED 已確認並停用；
     DISMISSED 管理員判斷此人仍在職，忽略（下次消失會重新累計）';

create index pending_deactivation_status_idx
    on integration_pending_deactivation (tenant_id, status);
