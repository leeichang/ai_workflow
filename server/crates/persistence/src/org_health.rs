//! 組織健康檢查
//!
//! 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §9。
//!
//! 目的不是「檢查資料完不完整」，而是回答一個具體問題：
//! **哪些人現在送單會卡住？**
//!
//! 因此每個檢查項都對應到 participant.rs 裡某個 resolver 的失敗路徑，
//! 而不是泛用的資料品質規則。檢查邏輯刻意與 resolver 的 SQL 對齊——
//! resolver 怎麼查，這裡就怎麼判。兩邊不一致的話，報表會說一切正常
//! 而流程照卡，那比沒有報表更糟。
//!
//! 可獨立重跑，不綁同步：MANUAL 維護的租戶同樣需要（§9 註）。

use crate::{Result, TenantTx};
use serde::Serialize;
use uuid::Uuid;

/// 嚴重度
///
/// HIGH 代表**流程會卡住**（resolver 解析回空，或指派給收不到的人）。
/// MEDIUM 代表功能受損但流程仍走得完。
///
/// 只有兩級，刻意不設 LOW：分不出輕重的等級會讓人忽略整張報表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    High,
    Medium,
}

/// 檢查項代碼
///
/// 用列舉而非字串：前端要依代碼決定「修正按鈕」跳到哪一頁，
/// 拼錯字串會變成沒有按鈕而不是編譯錯誤。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueCode {
    /// 員工沒有直屬主管。`manager_of` 解析回空
    EmployeeNoManager,
    /// 員工的主管已停用或已離職。`manager_of` 不過濾狀態，
    /// 會解析出這個人，任務指派給收不到信的人
    ManagerInactive,
    /// 主管鏈成環
    ManagerCycle,
    /// 部門沒有主管。`department_manager_of` 解析回空
    DepartmentNoManager,
    /// 部門主管自己是該部門成員時，`department_manager_of` 的
    /// `m.id <> u.id` 會排除自己——部門主管送單解析回空
    DepartmentManagerIsSelf,
    /// 員工沒有部門。依部門解析的 resolver 失效
    EmployeeNoDepartment,
    /// 員工 email 空白。`emails_of` 過濾掉空字串，通知靜默不寄
    ///
    /// 不檢查「email 重複」：`app_user` 有 `unique (tenant_id, email)`，
    /// 重複在資料庫層就進不來。檢查它只會是永遠不會命中的死碼
    EmployeeNoEmail,
    /// 部門的上層部門不存在。組織樹斷裂
    DepartmentBrokenParent,
}

impl IssueCode {
    /// 嚴重度由代碼決定，不讓呼叫端自行指定——
    /// 同一個問題在不同地方被標成不同等級會讓報表失去意義
    pub fn severity(self) -> Severity {
        match self {
            Self::EmployeeNoManager
            | Self::ManagerInactive
            | Self::ManagerCycle
            | Self::DepartmentNoManager
            | Self::DepartmentManagerIsSelf => Severity::High,
            Self::EmployeeNoDepartment
            | Self::EmployeeNoEmail
            | Self::DepartmentBrokenParent => Severity::Medium,
        }
    }

    /// 給使用者看的說明。點出**後果**而非現象——
    /// 「沒有主管」不會讓人想修，「送單會卡住」才會
    pub fn message(self) -> &'static str {
        match self {
            Self::EmployeeNoManager => "沒有直屬主管。使用 manager_of 的流程會在此人送單時卡住",
            Self::ManagerInactive => "直屬主管已停用或已離職。任務會指派給收不到通知的人",
            Self::ManagerCycle => "主管關係形成循環。多階簽核會無限迴圈",
            Self::DepartmentNoManager => {
                "部門沒有指定主管。使用 department_manager_of 的流程會卡住"
            }
            Self::DepartmentManagerIsSelf => {
                "此人是自己部門的主管。department_manager_of 會排除自己，送單時解析不到簽核人"
            }
            Self::EmployeeNoDepartment => "沒有所屬部門。依部門解析的簽核人找不到",
            Self::EmployeeNoEmail => "沒有 email。通知寄不出去，且不會有錯誤訊息",
            Self::DepartmentBrokenParent => "上層部門不存在。組織樹斷裂",
        }
    }
}

/// 一筆問題
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub severity: Severity,
    /// `employee` 或 `department`。前端據此決定跳到哪一頁
    pub subject_type: &'static str,
    pub subject_id: Uuid,
    /// 當下的顯示名。修正後報表重跑就會消失，
    /// 但保留名字讓報表本身可讀
    pub subject_name: String,
    pub message: &'static str,
    /// 補充資訊。例如重複的 email、循環的路徑
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Issue {
    fn new(
        code: IssueCode,
        subject_type: &'static str,
        subject_id: Uuid,
        subject_name: String,
    ) -> Self {
        Self {
            code,
            severity: code.severity(),
            subject_type,
            subject_id,
            subject_name,
            message: code.message(),
            detail: None,
        }
    }

    fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// 報表
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub high_count: usize,
    pub medium_count: usize,
    /// 檢查涵蓋的在職員工數。分母，讓「3 個問題」有比例可言
    pub employee_count: usize,
    pub department_count: usize,
    pub issues: Vec<Issue>,
}

/// 「活著的人」的判定條件
///
/// 離職者沒有主管不是問題——他不會送單。把他們算進來會讓報表
/// 充滿永遠不必修的項目，使用者就會開始忽略整張報表。
///
/// 做成函式而非常數：條件橫跨兩個欄位，有 self-join 的查詢裡
/// 每個欄位都要前綴。先前寫成常數再拼 `u.{...}`，只有第一個欄位
/// 拿到前綴，PostgreSQL 回 `column reference "left_at" is ambiguous`。
fn active_employee(alias: &str) -> String {
    format!(
        "{alias}.status = 'ACTIVE' \
         and ({alias}.left_at is null or {alias}.left_at > current_date)"
    )
}

pub async fn check(tx: &mut TenantTx<'_>) -> Result<HealthReport> {
    let mut issues = Vec::new();

    issues.extend(check_employee_manager(tx).await?);
    issues.extend(check_manager_cycle(tx).await?);
    issues.extend(check_department_manager(tx).await?);
    issues.extend(check_employee_department(tx).await?);
    issues.extend(check_email(tx).await?);
    issues.extend(check_department_parent(tx).await?);

    let employee_count: i64 = sqlx::query_scalar(&format!(
        "select count(*) from app_user u where {}",
        active_employee("u")
    ))
    .fetch_one(tx.executor())
    .await?;

    let department_count: i64 =
        sqlx::query_scalar("select count(*) from department where status = 'ACTIVE'")
            .fetch_one(tx.executor())
            .await?;

    let high_count = issues
        .iter()
        .filter(|i| i.severity == Severity::High)
        .count();
    let medium_count = issues.len() - high_count;

    // HIGH 排前面。使用者通常只修最上面幾項，順序決定他先修到什麼
    issues.sort_by_key(|i| (i.severity != Severity::High, i.subject_name.clone()));

    Ok(HealthReport {
        high_count,
        medium_count,
        employee_count: employee_count as usize,
        department_count: department_count as usize,
        issues,
    })
}

/// 直屬主管：沒有，或指向已停用 / 已離職者
///
/// 兩種情況一次查完。`manager_of` 的 join 條件只有 `m.id = u.manager_id`，
/// 完全不看 m 的狀態——所以「主管已離職」不會讓解析失敗，
/// 而是成功解析出一個收不到信的人。這比解析失敗更難發現。
async fn check_employee_manager(tx: &mut TenantTx<'_>) -> Result<Vec<Issue>> {
    let rows: Vec<(Uuid, String, Option<Uuid>, Option<String>, Option<bool>)> =
        sqlx::query_as(&format!(
            "select u.id, u.name, u.manager_id, m.name, ({}) \
             from app_user u \
             left join app_user m on m.id = u.manager_id \
             where {}",
            active_employee("m"),
            active_employee("u")
        ))
        .fetch_all(tx.executor())
        .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, name, manager_id, manager_name, manager_active)| {
            match (manager_id, manager_active) {
                (None, _) => Some(Issue::new(IssueCode::EmployeeNoManager, "employee", id, name)),
                (Some(_), Some(false)) => Some(
                    Issue::new(IssueCode::ManagerInactive, "employee", id, name).with_detail(
                        format!("主管：{}", manager_name.unwrap_or_else(|| "（已刪除）".into())),
                    ),
                ),
                // manager_id 指向不存在的列時 left join 回 null。
                // 外鍵約束保證不會發生，但不假設它一定在
                (Some(mid), None) => Some(
                    Issue::new(IssueCode::ManagerInactive, "employee", id, name)
                        .with_detail(format!("主管 {mid} 不存在")),
                ),
                _ => None,
            }
        })
        .collect())
}

/// 主管鏈成環
///
/// 用遞迴 CTE 而非在應用層爬圖：組織可能上萬人，一次查完比
/// N 次往返快。`cycle` 子句是 PostgreSQL 14 起的語法，
/// 它在偵測到重複節點時停止展開，避免無限遞迴。
async fn check_manager_cycle(tx: &mut TenantTx<'_>) -> Result<Vec<Issue>> {
    let rows: Vec<(Uuid, Vec<Uuid>)> = sqlx::query_as(&format!(
        r#"
        with recursive chain (root_id, current_id, depth, path, is_cycle) as (
            select u.id, u.manager_id, 1, array[u.id], false
            from app_user u
            where u.manager_id is not null and {active}
          union all
            select c.root_id, u.manager_id, c.depth + 1,
                   c.path || u.id, u.id = any(c.path)
            from chain c
            join app_user u on u.id = c.current_id
            where not c.is_cycle and c.depth < 50
        )
        select distinct on (root_id) root_id, path
        from chain
        where is_cycle
        order by root_id, depth
        "#,
        active = active_employee("u")
    ))
    .fetch_all(tx.executor())
    .await?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // 名字另外查。遞迴 CTE 裡 join 名字會讓每一層都多做一次 join
    let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
    let names: Vec<(Uuid, String)> =
        sqlx::query_as("select id, name from app_user where id = any($1)")
            .bind(&ids)
            .fetch_all(tx.executor())
            .await?;

    Ok(rows
        .into_iter()
        .map(|(id, path)| {
            let name = names
                .iter()
                .find(|(nid, _)| *nid == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_default();
            Issue::new(IssueCode::ManagerCycle, "employee", id, name)
                .with_detail(format!("循環長度 {}", path.len()))
        })
        .collect())
}

/// 部門主管：沒有指定，或指定的是部門成員自己
///
/// 第二種情況是需求 §9 的表格沒列到、但實務上最常發生的：
/// 部門主管自己送單。`department_manager_of` 有 `m.id <> u.id`
/// 排除自己（避免自己簽自己），結果是解析回空、流程卡住。
async fn check_department_manager(tx: &mut TenantTx<'_>) -> Result<Vec<Issue>> {
    let mut issues = Vec::new();

    let no_manager: Vec<(Uuid, String)> = sqlx::query_as(
        "select id, name from department \
         where status = 'ACTIVE' and manager_user_id is null",
    )
    .fetch_all(tx.executor())
    .await?;

    issues.extend(
        no_manager
            .into_iter()
            .map(|(id, name)| Issue::new(IssueCode::DepartmentNoManager, "department", id, name)),
    );

    // 主管本人屬於自己管的部門
    let self_managed: Vec<(Uuid, String, String)> = sqlx::query_as(&format!(
        "select u.id, u.name, d.name \
         from app_user u \
         join department d on d.id = u.department_id \
         where d.manager_user_id = u.id and d.status = 'ACTIVE' and {}",
        active_employee("u")
    ))
    .fetch_all(tx.executor())
    .await?;

    issues.extend(self_managed.into_iter().map(|(id, name, dept)| {
        Issue::new(IssueCode::DepartmentManagerIsSelf, "employee", id, name)
            .with_detail(format!("部門：{dept}"))
    }));

    Ok(issues)
}

async fn check_employee_department(tx: &mut TenantTx<'_>) -> Result<Vec<Issue>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(&format!(
        "select u.id, u.name from app_user u where {} and u.department_id is null",
        active_employee("u")
    ))
    .fetch_all(tx.executor())
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name)| Issue::new(IssueCode::EmployeeNoDepartment, "employee", id, name))
        .collect())
}

/// email 空白
///
/// 判定要與 `emails_of` 一致：它用 `email <> ''` 過濾，
/// 所以空字串收不到信。欄位本身 `not null`，不必檢查 NULL。
async fn check_email(tx: &mut TenantTx<'_>) -> Result<Vec<Issue>> {
    let blank: Vec<(Uuid, String)> = sqlx::query_as(&format!(
        "select u.id, u.name from app_user u \
         where {} and coalesce(u.email, '') = ''",
        active_employee("u")
    ))
    .fetch_all(tx.executor())
    .await?;

    Ok(blank
        .into_iter()
        .map(|(id, name)| Issue::new(IssueCode::EmployeeNoEmail, "employee", id, name))
        .collect())
}

async fn check_department_parent(tx: &mut TenantTx<'_>) -> Result<Vec<Issue>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "select d.id, d.name from department d \
         left join department p on p.id = d.parent_id \
         where d.status = 'ACTIVE' and d.parent_id is not null and p.id is null",
    )
    .fetch_all(tx.executor())
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name)| Issue::new(IssueCode::DepartmentBrokenParent, "department", id, name))
        .collect())
}
