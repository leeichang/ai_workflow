-- 流程實例鎖定表單版本
--
-- 為何需要這份 migration：
--   workflow_instance 已經鎖定流程版本（workflow_version_id，0005:21），
--   但表單沒有對應欄位。流程跑到一半改表單定義，進行中的實例
--   會用到新版——欄位被刪掉、必填變選填、權限改了，
--   都會直接影響還沒簽完的單。
--
--   需求 03 §3.5 宣稱「進行中的實例不受還原影響」，
--   在表單沒有版本鎖定的前提下那個宣稱不成立。
--
--   沙箱（S1）與 AI 產生器都會大量改表單定義，這個缺口會被放大，
--   所以列為 S1 的硬前置。
--
-- 哪一張表單：
--   由表單自己宣告服務哪個業務物件（form_definition.business_object），
--   實例啟動時以流程的 business_object 反查。
--   方向是「表單指定業務物件」，不是「流程指定表單」——
--   後者會讓同一張表單被多個流程各自宣告一次，遲早不一致。

alter table workflow_instance
  add column form_version_id uuid
      references form_definition_version(id) on delete restrict;

comment on column workflow_instance.form_version_id is
    '實例建立當下的表單版本。與 workflow_version_id 同樣用 restrict——
     進行中的單所依賴的定義不該被刪除。
     可為 NULL：既有實例沒有這個值，且業務物件未必有已發布的表單';

-- 依實例反查表單版本是渲染單據的必經路徑，建索引避免全表掃描
create index if not exists workflow_instance_form_version_idx
    on workflow_instance (form_version_id)
    where form_version_id is not null;
