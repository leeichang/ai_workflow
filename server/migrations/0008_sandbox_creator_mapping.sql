-- 沙箱建立者在沙箱內的對應 id
--
-- 為何需要：app_user.id 是全域主鍵，沙箱複製主檔時必須重新產生 id。
-- 結果是同一個人在正式租戶與沙箱裡是兩個不同的 uuid。
--
-- sandbox_session.created_by 記的是**正式租戶**的 id（那是真實的人，
-- 稽核要指得回去）。但測試者登入沙箱之後，JWT 帶的是**沙箱**的 id。
--
-- 授權開洞的條件是「操作者必須是沙箱的建立者本人」，兩邊用不同的
-- id 比對必定不成立——建立者反而無法在自己的沙箱裡模擬簽核。
--
-- 因此兩個都記：created_by 給稽核用，created_by_in_sandbox 給授權用。

alter table sandbox_session
  add column created_by_in_sandbox uuid;

comment on column sandbox_session.created_by_in_sandbox is
    '建立者在沙箱租戶內的 user id。授權開洞比對的是這個，
     因為登入沙箱後 JWT 帶的是沙箱的 id。
     created_by 則是正式租戶的 id，稽核用';
