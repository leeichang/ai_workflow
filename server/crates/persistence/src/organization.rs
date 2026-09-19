//! 組織資料存取
//!
//! 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §3、§4。
//!
//! 這一層的核心不是 CRUD，而是 **欄位級 ownership**（§3）：
//! 管理員編輯過的欄位要記進 `platform_managed_fields`，
//! 日後同步時一律跳過。沒有這個機制，D-14「ERP 優先、平台補缺」
//! 是空話——管理員補的資料會在下一次同步被沖掉。
//!
//! 目前 `source_system` 全是 `MANUAL`（同步機制尚未實作），
//! 但編輯**照樣記錄**。日後某個人從 MANUAL 轉為 ERP 同步來源時，
//! 他先前的編輯自動受保護；若等到那時才補記錄，中間的編輯就沒有保護。
//!
//! ## 為什麼 patch 帶明確的欄位清單
//!
//! PATCH 要能表達「把主管清成空」。單純用 `Option<T>` 做不到——
//! 送 `{"manager_id": null}` 與完全不送這個 key，反序列化後都是 `None`。
//!
//! 常見解法是 `Option<Option<T>>`（double option），但那需要額外的
//! serde 依賴，且欄位變多時很難讀。這裡改用明確的 `fields` 清單：
//! 呼叫端說「我要改 manager_id 與 job_title」，值從同一個物件取。
//!
//! 這個設計正好也是 `platform_managed_fields` 需要的資訊——
//! 「哪些欄位被改了」本來就要記錄，兩件事合一，不必推導。

use crate::{Error, Result, TenantTx};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── 部門 ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Department {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub manager_user_id: Option<Uuid>,
    /// 主管的顯示名。前端要顯示「誰在管」，不該為此再查一次
    pub manager_name: Option<String>,
    pub status: String,
    pub sort_order: i32,
    pub source_system: String,
    pub platform_managed_fields: Vec<String>,
    /// 直屬成員數（不含子部門）。空部門在樹上要看得出來
    pub member_count: i64,
}

/// 列出全部部門
///
/// 不在 SQL 裡組樹：部門數量是百位數等級，在應用層組比遞迴 CTE
/// 好維護，而且前端本來就要拿到扁平清單做「上層部門」下拉選單。
pub async fn list_departments(tx: &mut TenantTx<'_>) -> Result<Vec<Department>> {
    let rows = sqlx::query_as::<_, Department>(
        r#"
        select d.id, d.code, d.name, d.parent_id, d.manager_user_id,
               m.name as manager_name,
               d.status, d.sort_order, d.source_system,
               d.platform_managed_fields,
               (select count(*) from app_user u where u.department_id = d.id)
                   as member_count
        from department d
        left join app_user m on m.id = d.manager_user_id
        order by d.sort_order, d.code
        "#,
    )
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

#[derive(Debug, Deserialize)]
pub struct NewDepartment {
    pub code: String,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub manager_user_id: Option<Uuid>,
    pub sort_order: Option<i32>,
}

/// 建立部門
///
/// `source_system` 固定 `MANUAL`——這是後台建的，不屬於任何來源系統。
/// 建立時不寫 `platform_managed_fields`：整筆都是平台的，
/// 逐欄位標記沒有意義（§5.1「MANUAL 所有欄位視同 platform_managed_fields」）。
pub async fn create_department(tx: &mut TenantTx<'_>, input: &NewDepartment) -> Result<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "insert into department
             (tenant_id, code, name, parent_id, manager_user_id, sort_order, source_system)
         values ($1, $2, $3, $4, $5, coalesce($6, 0), 'MANUAL')
         returning id",
    )
    .bind(tx.tenant_id())
    .bind(&input.code)
    .bind(&input.name)
    .bind(input.parent_id)
    .bind(input.manager_user_id)
    .bind(input.sort_order)
    .fetch_one(tx.executor())
    .await
    // 部門代碼重複時要回「代碼已被使用」而非原始 SQL 錯誤，
    // 前端才知道是使用者填錯而不是系統壞了
    .map_err(Error::from_db)?;

    Ok(id)
}

/// 部門的編輯
///
/// `fields` 列出這次要改哪些欄位，值從對應的欄位取。
/// 不在 `fields` 裡的欄位一律不動，即使物件裡帶了值。
#[derive(Debug, Default, Deserialize)]
pub struct DepartmentPatch {
    pub fields: Vec<String>,
    pub name: Option<String>,
    pub parent_id: Option<Uuid>,
    pub manager_user_id: Option<Uuid>,
    pub status: Option<String>,
    pub sort_order: Option<i32>,
}

/// 部門可編輯的欄位白名單
///
/// 寫死而非從結構推導：欄位名會進 `platform_managed_fields` 並存進
/// 資料庫，等同 API 契約。允許任意字串會讓同步邏輯日後讀到不認得的名字。
///
/// `code` 不可改：它是 `unique (tenant_id, code)` 的一部分，
/// 也是同步匹配的備援鍵，改了會讓既有的對應關係斷掉。
const DEPARTMENT_EDITABLE: &[&str] =
    &["name", "parent_id", "manager_user_id", "status", "sort_order"];

pub async fn update_department(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    patch: &DepartmentPatch,
) -> Result<()> {
    validate_fields(&patch.fields, DEPARTMENT_EDITABLE)?;

    if patch.fields.is_empty() {
        return Ok(());
    }

    // 成環會讓部門樹在前端無限展開，也會讓未來的階層查詢死迴圈
    if patch.fields.iter().any(|f| f == "parent_id") {
        assert_no_department_cycle(tx, id, patch.parent_id).await?;
    }

    // 逐欄位更新。動態拼 SQL 只拼欄位名，且欄位名已通過白名單，
    // 值一律走參數綁定
    for field in &patch.fields {
        let sql = format!("update department set {field} = $2, updated_at = now() where id = $1");
        let query = sqlx::query(&sql).bind(id);

        let result = match field.as_str() {
            "name" => query.bind(&patch.name).execute(tx.executor()).await,
            "parent_id" => query.bind(patch.parent_id).execute(tx.executor()).await,
            "manager_user_id" => {
                query
                    .bind(patch.manager_user_id)
                    .execute(tx.executor())
                    .await
            }
            "status" => query.bind(&patch.status).execute(tx.executor()).await,
            "sort_order" => query.bind(patch.sort_order).execute(tx.executor()).await,
            // validate_fields 已擋下，這裡到不了
            other => return Err(Error::InvalidSchema(format!("不可編輯的欄位：{other}"))),
        };

        result.map_err(Error::from_db)?;
    }

    mark_platform_managed(tx, "department", id, &patch.fields).await?;
    Ok(())
}

/// 刪除部門
///
/// **只刪得掉沒有成員、也沒有子部門的部門。**
///
/// 需求 §7 說「來源系統查不到的人不刪除，改標記 status」，那是針對
/// **同步**的規則——硬刪會讓進行中流程的 assignee 變成孤兒。
/// 但使用者手誤建了一個空部門時應該刪得掉，否則樹上會永遠留著垃圾。
///
/// 兩個 FK（`department.parent_id`、`app_user.department_id`）都是
/// `on delete set null`。若允許刪除有成員的部門，那些人的
/// `department_id` 會被**靜默清空**——依部門解析的簽核人全部失效，
/// 而且沒有任何錯誤訊息。擋在前面而不是靠 FK 的預設行為。
pub async fn delete_department(tx: &mut TenantTx<'_>, id: Uuid) -> Result<()> {
    let members: i64 =
        sqlx::query_scalar("select count(*) from app_user where department_id = $1")
            .bind(id)
            .fetch_one(tx.executor())
            .await?;

    if members > 0 {
        return Err(Error::Conflict(format!(
            "這個部門還有 {members} 位成員。請先把他們移到其他部門，\
             或將部門改為停用"
        )));
    }

    let children: i64 =
        sqlx::query_scalar("select count(*) from department where parent_id = $1")
            .bind(id)
            .fetch_one(tx.executor())
            .await?;

    if children > 0 {
        return Err(Error::Conflict(format!(
            "這個部門底下還有 {children} 個子部門"
        )));
    }

    let affected = sqlx::query("delete from department where id = $1")
        .bind(id)
        .execute(tx.executor())
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(Error::not_found("department", id));
    }

    Ok(())
}

/// 部門不可成為自己的祖先
///
/// 只在改 `parent_id` 時檢查。往上爬而非往下找子孫：
/// 往上的路徑長度是樹高（個位數），往下要掃整棵子樹。
async fn assert_no_department_cycle(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    new_parent: Option<Uuid>,
) -> Result<()> {
    let Some(parent) = new_parent else {
        return Ok(());
    };

    if parent == id {
        return Err(Error::Conflict("上層部門不能是自己".into()));
    }

    let mut cursor = Some(parent);
    // 樹高上限。資料已經成環時（不該發生）才會用到，避免無限迴圈
    for _ in 0..50 {
        let Some(current) = cursor else {
            return Ok(());
        };
        if current == id {
            return Err(Error::Conflict(
                "不能把部門移到自己的下層部門底下".into(),
            ));
        }
        cursor = sqlx::query_scalar("select parent_id from department where id = $1")
            .bind(current)
            .fetch_optional(tx.executor())
            .await?
            .flatten();
    }

    Err(Error::Conflict("部門階層過深或已經成環".into()))
}

// ── 員工 ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Employee {
    pub id: Uuid,
    pub employee_no: Option<String>,
    pub name: String,
    pub email: String,
    pub department_id: Option<Uuid>,
    pub department_name: Option<String>,
    pub manager_id: Option<Uuid>,
    pub manager_name: Option<String>,
    pub job_title: Option<String>,
    pub phone: Option<String>,
    pub extension: Option<String>,
    pub hired_at: Option<chrono::NaiveDate>,
    pub left_at: Option<chrono::NaiveDate>,
    pub status: String,
    pub can_login: bool,
    pub sync_status: String,
    pub source_system: String,
    pub platform_managed_fields: Vec<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EmployeeFilter {
    /// 關鍵字。比對姓名、email、員工編號
    pub q: Option<String>,
    pub department_id: Option<Uuid>,
    /// 預設只列在職者。要看離職的人必須明講
    pub include_inactive: Option<bool>,
}

/// 員工查詢的共用 select
///
/// 抽成常數而非各自重寫：`roles` 的子查詢與兩個 left join
/// 在清單與單筆之間必須一致，否則同一個人在兩個畫面顯示的
/// 部門或角色會不一樣。
const EMPLOYEE_SELECT: &str = r#"
    select u.id, u.employee_no, u.name, u.email,
           u.department_id, d.name as department_name,
           u.manager_id, m.name as manager_name,
           u.job_title, u.phone, u.extension,
           u.hired_at, u.left_at, u.status, u.can_login,
           u.sync_status, u.source_system, u.platform_managed_fields,
           array(
               select r.code from user_role ur
               join role r on r.id = ur.role_id
               where ur.user_id = u.id
               order by r.code
           ) as roles
    from app_user u
    left join department d on d.id = u.department_id
    left join app_user m on m.id = u.manager_id
"#;

/// 取單一員工
///
/// 改派待辦前要確認對象還在職——離職的人查得到（RLS 只隔離租戶），
/// 改派給他等於把單丟進黑洞。
pub async fn find_employee(tx: &mut TenantTx<'_>, id: Uuid) -> Result<Employee> {
    sqlx::query_as::<_, Employee>(&format!("{EMPLOYEE_SELECT} where u.id = $1"))
        .bind(id)
        .fetch_optional(tx.executor())
        .await?
        .ok_or_else(|| crate::Error::not_found("app_user", id))
}

pub async fn list_employees(
    tx: &mut TenantTx<'_>,
    filter: &EmployeeFilter,
) -> Result<Vec<Employee>> {
    let pattern = filter.q.as_deref().map(|s| format!("%{s}%"));

    let rows = sqlx::query_as::<_, Employee>(&format!(
        r#"
        {EMPLOYEE_SELECT}
        where ($1::boolean is true
               or (u.status = 'ACTIVE'
                   and (u.left_at is null or u.left_at > current_date)))
          and ($2::uuid is null or u.department_id = $2)
          and ($3::text is null
               or u.name ilike $3
               or u.email ilike $3
               or coalesce(u.employee_no, '') ilike $3)
        order by coalesce(u.employee_no, ''), u.name
        "#
    ))
    .bind(filter.include_inactive.unwrap_or(false))
    .bind(filter.department_id)
    .bind(pattern)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 刪除員工
///
/// **只刪得掉從未參與任何流程、也沒有被任何東西指向的人。**
///
/// 需求 §7 說「永不硬刪」，那是針對**同步**的規則——來源系統查不到
/// 不代表這個人離職了。但誤建的人、或匯入測試留下的資料應該收得回去，
/// 否則組織清單會越來越髒。
///
/// `app_user` 有 19 個外鍵指向它，多數是 `on delete set null` 的稽核
/// 欄位（`created_by`、`published_by`）。真正會出事的是這幾個：
///
///   - `human_task.assignee_user_id`：待辦變成沒有人能處理的孤兒
///   - `workflow_instance.started_by`：誰送的單查不到
///   - `app_user.manager_id` / `department.manager_user_id`：
///     簽核人解析會突然變成空
///   - `approval_delegation`：是 cascade，代理設定會被連帶刪掉
///
/// 這些全部擋在前面，而不是靠 FK 的預設行為——`set null` 是靜默的，
/// 出事時沒有任何訊息。
pub async fn delete_employee(tx: &mut TenantTx<'_>, id: Uuid) -> Result<()> {
    // (檢查用的 SQL, 擋下時的說明)
    let blockers: [(&str, &str); 5] = [
        (
            "select count(*) from human_task where assignee_user_id = $1",
            "此人有待辦任務。刪除會讓這些任務變成沒有人能處理",
        ),
        (
            "select count(*) from workflow_instance where started_by = $1",
            "此人送過單。刪除會讓那些流程查不到申請人",
        ),
        (
            "select count(*) from app_user where manager_id = $1",
            "此人是其他員工的直屬主管。請先改派他們的主管",
        ),
        (
            "select count(*) from department where manager_user_id = $1",
            "此人是某個部門的主管。請先改派該部門的主管",
        ),
        (
            "select count(*) from approval_delegation
             where delegator_id = $1 or delegate_id = $1",
            "此人有簽核代理設定。請先移除",
        ),
    ];

    for (sql, message) in blockers {
        let count: i64 = sqlx::query_scalar(sql)
            .bind(id)
            .fetch_one(tx.executor())
            .await?;

        if count > 0 {
            return Err(Error::Conflict(format!("{message}（{count} 筆）")));
        }
    }

    // 角色指派是 cascade，跟著刪掉沒有問題——它只是這個人的權限
    let affected = sqlx::query("delete from app_user where id = $1")
        .bind(id)
        .execute(tx.executor())
        .await
        .map_err(Error::from_db)?
        .rows_affected();

    if affected == 0 {
        return Err(Error::not_found("app_user", id));
    }

    Ok(())
}

/// 員工的編輯
#[derive(Debug, Default, Deserialize)]
pub struct EmployeePatch {
    pub fields: Vec<String>,
    pub name: Option<String>,
    pub employee_no: Option<String>,
    pub department_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub job_title: Option<String>,
    pub phone: Option<String>,
    pub extension: Option<String>,
    pub hired_at: Option<chrono::NaiveDate>,
    pub left_at: Option<chrono::NaiveDate>,
    pub status: Option<String>,
}

/// 員工可編輯的欄位白名單
///
/// 刻意**不含**：
///   - `email`：它是登入帳號，改它等於換人
///   - `can_login` 與密碼：牽涉登入安全，要走獨立端點才看得出誰改了什麼
///   - 角色：那是權限問題，不是組織資料
const EMPLOYEE_EDITABLE: &[&str] = &[
    "name",
    "employee_no",
    "department_id",
    "manager_id",
    "job_title",
    "phone",
    "extension",
    "hired_at",
    "left_at",
    "status",
];

pub async fn update_employee(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    patch: &EmployeePatch,
) -> Result<()> {
    validate_fields(&patch.fields, EMPLOYEE_EDITABLE)?;

    if patch.fields.is_empty() {
        return Ok(());
    }

    if patch.fields.iter().any(|f| f == "manager_id") {
        assert_no_manager_cycle(tx, id, patch.manager_id).await?;
    }

    for field in &patch.fields {
        let sql = format!("update app_user set {field} = $2, updated_at = now() where id = $1");
        let query = sqlx::query(&sql).bind(id);

        let result = match field.as_str() {
            "name" => query.bind(&patch.name).execute(tx.executor()).await,
            "employee_no" => {
                // 空字串會讓 unique index 把兩個「沒填」的人視為衝突。
                // 統一存成 null
                let value = patch.employee_no.as_deref().filter(|s| !s.is_empty());
                query.bind(value).execute(tx.executor()).await
            }
            "department_id" => query.bind(patch.department_id).execute(tx.executor()).await,
            "manager_id" => query.bind(patch.manager_id).execute(tx.executor()).await,
            "job_title" => query.bind(&patch.job_title).execute(tx.executor()).await,
            "phone" => query.bind(&patch.phone).execute(tx.executor()).await,
            "extension" => query.bind(&patch.extension).execute(tx.executor()).await,
            "hired_at" => query.bind(patch.hired_at).execute(tx.executor()).await,
            "left_at" => query.bind(patch.left_at).execute(tx.executor()).await,
            "status" => query.bind(&patch.status).execute(tx.executor()).await,
            other => return Err(Error::InvalidSchema(format!("不可編輯的欄位：{other}"))),
        };

        // employee_no 重複、status 不合 check 約束等都在這裡轉成
        // 有意義的訊息。直接回原始 SQL 錯誤前端無法判斷該怎麼處理
        result.map_err(Error::from_db)?;
    }

    mark_platform_managed(tx, "app_user", id, &patch.fields).await?;
    Ok(())
}

/// 主管鏈不可成環
///
/// 成環會讓多階簽核無限迴圈。組織健康檢查會報出既有的環，
/// 但編輯時就該擋下——讓使用者存得進去再叫他去修沒有道理。
async fn assert_no_manager_cycle(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    new_manager: Option<Uuid>,
) -> Result<()> {
    let Some(manager) = new_manager else {
        return Ok(());
    };

    if manager == id {
        return Err(Error::Conflict("直屬主管不能是自己".into()));
    }

    let mut cursor = Some(manager);
    for _ in 0..50 {
        let Some(current) = cursor else {
            return Ok(());
        };
        if current == id {
            return Err(Error::Conflict(
                "不能指定自己的下屬為主管，會讓簽核循環".into(),
            ));
        }
        cursor = sqlx::query_scalar("select manager_id from app_user where id = $1")
            .bind(current)
            .fetch_optional(tx.executor())
            .await?
            .flatten();
    }

    Err(Error::Conflict("主管鏈過深或已經成環".into()))
}

// ── 欄位級 ownership ────────────────────────────────────

/// 欄位白名單驗證
///
/// 用 `InvalidSchema` 而非 `Conflict`：這是輸入不合法（400），
/// 不是與現有資料衝突（409）。前端據此決定要不要重試。
fn validate_fields(fields: &[String], allowed: &'static [&'static str]) -> Result<()> {
    for field in fields {
        if !allowed.contains(&field.as_str()) {
            return Err(Error::InvalidSchema(format!(
                "不可編輯的欄位「{field}」。可編輯的欄位：{}",
                allowed.join("、")
            )));
        }
    }
    Ok(())
}

/// 把編輯過的欄位記進 `platform_managed_fields`
///
/// 用 `array(select distinct unnest(...))` 而非直接 `||`：
/// 同一個欄位改第二次時不該在陣列裡出現兩筆。
///
/// `table` 只可能是本模組寫死的兩個值，不來自外部輸入。
async fn mark_platform_managed(
    tx: &mut TenantTx<'_>,
    table: &'static str,
    id: Uuid,
    fields: &[String],
) -> Result<()> {
    let sql = format!(
        "update {table}
         set platform_managed_fields = array(
             select distinct unnest(platform_managed_fields || $2::text[])
         )
         where id = $1"
    );

    sqlx::query(&sql)
        .bind(id)
        .bind(fields)
        .execute(tx.executor())
        .await?;

    Ok(())
}

/// 解除欄位鎖定，改回跟隨來源系統
///
/// 需求 §3 規則 3。從陣列移除後，下一次同步就會覆蓋這個欄位。
///
/// 不驗證欄位是否在白名單內：移除一個不存在的欄位是無害的，
/// 而且日後若白名單縮減，既有的鎖定仍要解得掉。
pub async fn unlock_fields(
    tx: &mut TenantTx<'_>,
    table: &'static str,
    id: Uuid,
    fields: &[String],
) -> Result<()> {
    let sql = format!(
        "update {table}
         set platform_managed_fields = array(
             select f from unnest(platform_managed_fields) as f
             where f <> all($2::text[])
         )
         where id = $1"
    );

    sqlx::query(&sql)
        .bind(id)
        .bind(fields)
        .execute(tx.executor())
        .await?;

    Ok(())
}
