//! 組織資料同步
//!
//! 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §5、§7。
//!
//! 管線的後半段：Validate → Upsert。前半段（Parse、Map）在
//! `domain::import`，它不碰資料庫。
//!
//! ## 這個模組最重要的三件事
//!
//! 1. **完整性閘（§7 規則 2）**。來源 1000 人只回 823 人時，若直接
//!    把差額當成離職，其中只要有一個是主管，全公司相關流程立刻卡死。
//!    低於門檻就整批中止，不寫入任何變更。
//!
//! 2. **欄位級 ownership（§3）**。`platform_managed_fields` 列出的
//!    欄位一律跳過。沒有這個，管理員在平台補的資料會被沖掉。
//!
//! 3. **永不硬刪（§7 規則 1）**。來源消失的人標記 `MISSING_IN_SOURCE`，
//!    `status` 維持不變。硬刪會讓進行中流程的 assignee 變成孤兒。
//!
//! ## 寫入順序
//!
//! §5.3 指出的陷阱：`department.parent_id`、`department.manager_user_id`、
//! `app_user.manager_id` 三個都是自參照，一次 upsert 會踩到還不存在的 id。
//! 必須分階段，見 [`sync_employees`]。

use crate::{Error, Result, TenantTx};
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

/// 映射後的一列
///
/// 結構與 `domain::import::MappedRow` 相同，但**刻意不共用型別**：
/// persistence 是資料存取層，不該依賴 domain。前者的職責是
/// 「把 canonical 的列寫進資料庫」，來源是 CSV、Excel 還是 REST API
/// 都不該影響它。
///
/// 轉換由 http 層做——那裡本來就同時看得到兩邊。
#[derive(Debug, Clone)]
pub struct SyncRow {
    /// 來源的第幾列（1-based，不含標題列）
    pub row_number: usize,
    /// canonical 欄位名 → 原始字串
    pub values: HashMap<String, String>,
}

/// §10.3 的計數契約
///
/// 每一個都要有——同步不能只回「success」，管理員要看得出
/// 兩邊差異有多大。
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncCounts {
    pub received_count: i32,
    pub parsed_count: i32,
    pub mapped_count: i32,
    pub validated_count: i32,
    pub inserted_count: i32,
    pub updated_count: i32,
    pub unchanged_count: i32,
    /// 因 platform_managed_fields 跳過的**欄位**數，不是筆數
    pub skipped_by_ownership_count: i32,
    pub rejected_count: i32,
    pub missing_in_source_count: i32,
}

/// 單筆問題
#[derive(Debug, Clone, Serialize)]
pub struct SyncIssue {
    pub row_number: Option<i32>,
    pub kind: &'static str,
    pub message: String,
    pub field: Option<String>,
    pub matched_ids: Vec<Uuid>,
}

/// 同步結果
#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    pub status: &'static str,
    pub counts: SyncCounts,
    pub issues: Vec<SyncIssue>,
    pub message: Option<String>,
}

/// 完整性閘的判定
///
/// 分離出來是為了能單獨測試——這是整個同步最危險的一個判斷，
/// 而要用真實資料庫重現「來源突然少一半」的情境很麻煩。
///
/// `last_success` 為 None 代表從未成功同步過，首次匯入沒有比較基準，
/// 閘不生效。
pub fn passes_completeness_gate(
    received: i32,
    last_success: Option<i32>,
    threshold: f64,
) -> bool {
    let Some(last) = last_success else {
        return true;
    };
    if last <= 0 {
        return true;
    }
    f64::from(received) >= f64::from(last) * threshold
}

// ── 驗證 ────────────────────────────────────────────────

/// 驗證後的員工列
struct ValidEmployee {
    row_number: i32,
    employee_no: String,
    name: Option<String>,
    email: Option<String>,
    department_code: Option<String>,
    manager_employee_no: Option<String>,
    job_title: Option<String>,
    phone: Option<String>,
    extension: Option<String>,
    hired_at: Option<NaiveDate>,
    left_at: Option<NaiveDate>,
    external_id: Option<String>,
}

/// 日期的容錯解析
///
/// 客戶的 Excel 日期格式不會統一。少數幾種常見格式硬編比要求
/// 客戶統一格式實際——後者在導入期會變成來回往返。
fn parse_date(value: &str) -> Option<NaiveDate> {
    const FORMATS: &[&str] = &["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d", "%d/%m/%Y", "%m/%d/%Y"];

    FORMATS
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(value, f).ok())
}

/// 驗證員工列
///
/// `employee_no` 是必要的——它是匹配鍵（§5.2 的第二順位），
/// 沒有它就無法判斷「這是新人還是既有的人」，只能每次都新建，
/// 同步兩次就會有兩份。
fn validate_employees(rows: &[SyncRow]) -> (Vec<ValidEmployee>, Vec<SyncIssue>) {
    let mut valid = Vec::new();
    let mut issues = Vec::new();

    for row in rows {
        let row_number = row.row_number as i32;

        let Some(employee_no) = row.values.get("employee_no").filter(|s| !s.is_empty())
        else {
            issues.push(SyncIssue {
                row_number: Some(row_number),
                kind: "VALIDATION_FAILED",
                message: "缺少員工編號。沒有它無法判斷是新人還是既有員工".into(),
                field: Some("employee_no".into()),
                matched_ids: Vec::new(),
            });
            continue;
        };

        // 日期格式錯誤不整列退掉——一個打錯的離職日不該讓
        // 這個人的姓名與部門也同步不進來
        let mut date_of = |key: &str| -> Option<NaiveDate> {
            let raw = row.values.get(key)?;
            match parse_date(raw) {
                Some(d) => Some(d),
                None => {
                    issues.push(SyncIssue {
                        row_number: Some(row_number),
                        kind: "VALIDATION_FAILED",
                        message: format!("日期格式無法辨識：「{raw}」"),
                        field: Some(key.to_string()),
                        matched_ids: Vec::new(),
                    });
                    None
                }
            }
        };

        let hired_at = date_of("hired_at");
        let left_at = date_of("left_at");

        valid.push(ValidEmployee {
            row_number,
            employee_no: employee_no.clone(),
            name: row.values.get("name").cloned(),
            email: row.values.get("email").cloned(),
            department_code: row.values.get("department_code").cloned(),
            manager_employee_no: row.values.get("manager_employee_no").cloned(),
            job_title: row.values.get("job_title").cloned(),
            phone: row.values.get("phone").cloned(),
            extension: row.values.get("extension").cloned(),
            hired_at,
            left_at,
            external_id: row.values.get("external_id").cloned(),
        });
    }

    (valid, issues)
}

// ── 同步員工 ────────────────────────────────────────────

/// 既有員工的關鍵欄位，供比對與 ownership 檢查
struct ExistingEmployee {
    id: Uuid,
    platform_managed_fields: Vec<String>,
}

/// 同步員工
///
/// 依 §5.3 分階段：
///   Phase 1  upsert app_user（含 department_id，不含 manager_id）
///   Phase 2  回填 manager_id
///
/// 部門必須先同步好——員工的 `department_code` 要查得到對應的部門。
///
/// `dry_run` 時完全不寫入，但計數照算。管理員要能在真的寫進去之前
/// 看到「會新增 12 筆、更新 340 筆、跳過 8 個欄位」。
pub async fn sync_employees(
    tx: &mut TenantTx<'_>,
    source_id: Uuid,
    rows: &[SyncRow],
    last_success_count: Option<i32>,
    threshold: f64,
    dry_run: bool,
) -> Result<SyncOutcome> {
    let mut counts = SyncCounts {
        received_count: rows.len() as i32,
        parsed_count: rows.len() as i32,
        mapped_count: rows.len() as i32,
        ..Default::default()
    };

    let (valid, mut issues) = validate_employees(rows);
    counts.validated_count = valid.len() as i32;
    counts.rejected_count = (rows.len() - valid.len()) as i32;

    // 完整性閘用**通過驗證的筆數**而非讀到的筆數：
    // 一份格式全壞的檔案讀得到 1000 列卻一筆都寫不進去，
    // 那與「來源只回 100 筆」一樣危險
    if !passes_completeness_gate(counts.validated_count, last_success_count, threshold) {
        let last = last_success_count.unwrap_or(0);
        let validated = counts.validated_count;
        return Ok(SyncOutcome {
            status: "ABORTED_INCOMPLETE",
            counts,
            issues,
            message: Some(format!(
                "本次只有 {validated} 筆通過驗證，低於上次成功同步的 {last} 筆 × {:.0}%。\
                 整批中止，未寫入任何變更——來源資料不完整時若照寫，\
                 消失的人會被當成離職",
                threshold * 100.0
            )),
        });
    }

    // 部門代碼 → id。員工的 department_id 要靠它解析
    let departments: HashMap<String, Uuid> =
        sqlx::query_as::<_, (String, Uuid)>("select code, id from department")
            .fetch_all(tx.executor())
            .await?
            .into_iter()
            .collect();

    // 既有員工，用 employee_no 匹配（§5.2 的第二順位）
    let existing: HashMap<String, ExistingEmployee> = sqlx::query_as::<_, (Uuid, String, Vec<String>)>(
        "select id, employee_no, platform_managed_fields from app_user
         where employee_no is not null",
    )
    .fetch_all(tx.executor())
    .await?
    .into_iter()
    .map(|(id, no, managed)| {
        (
            no,
            ExistingEmployee {
                id,
                platform_managed_fields: managed,
            },
        )
    })
    .collect();

    let mut seen: Vec<String> = Vec::new();

    // ── Phase 1：upsert，不含 manager_id ──
    for row in &valid {
        seen.push(row.employee_no.clone());

        let department_id = row
            .department_code
            .as_ref()
            .and_then(|code| departments.get(code).copied());

        // 部門代碼查不到要告訴管理員。靜默忽略的話，那個人會
        // 沒有部門，而依部門解析的簽核人就找不到
        if let Some(code) = &row.department_code {
            if department_id.is_none() {
                issues.push(SyncIssue {
                    row_number: Some(row.row_number),
                    kind: "VALIDATION_FAILED",
                    message: format!("找不到部門代碼「{code}」。請先同步部門，或確認代碼正確"),
                    field: Some("department_code".into()),
                    matched_ids: Vec::new(),
                });
            }
        }

        match existing.get(&row.employee_no) {
            Some(current) => {
                let (changed, skipped) = update_employee(
                    tx,
                    current,
                    row,
                    department_id,
                    dry_run,
                    &mut issues,
                )
                .await?;

                counts.skipped_by_ownership_count += skipped;
                if changed {
                    counts.updated_count += 1;
                } else {
                    counts.unchanged_count += 1;
                }
            }
            None => {
                if !dry_run {
                    insert_employee(tx, source_id, row, department_id).await?;
                }
                counts.inserted_count += 1;
            }
        }
    }

    // ── Phase 2：回填 manager_id ──
    //
    // 必須等所有人都建好。第一次匯入時主管可能排在部屬後面，
    // 提前回填會查不到人
    if !dry_run {
        for row in &valid {
            let Some(manager_no) = &row.manager_employee_no else {
                continue;
            };

            let updated = sqlx::query(
                "update app_user u set manager_id = m.id, updated_at = now()
                 from app_user m
                 where u.employee_no = $1 and m.employee_no = $2
                   and u.id <> m.id
                   and not ('manager_id' = any(u.platform_managed_fields))",
            )
            .bind(&row.employee_no)
            .bind(manager_no)
            .execute(tx.executor())
            .await?
            .rows_affected();

            if updated == 0 {
                // 分不出是「主管不存在」還是「欄位被平台接管」，
                // 兩者都要讓管理員知道
                issues.push(SyncIssue {
                    row_number: Some(row.row_number),
                    kind: "VALIDATION_FAILED",
                    message: format!(
                        "主管員工編號「{manager_no}」未套用。可能是該員工不在本次資料中，\
                         或此欄位已被平台接管"
                    ),
                    field: Some("manager_id".into()),
                    matched_ids: Vec::new(),
                });
            }
        }
    }

    // ── 來源消失的人（§7 規則 1）──
    //
    // 永不硬刪。標記 MISSING_IN_SOURCE，status 維持不變。
    //
    // 找曾經同步過的人。包含**已經是 MISSING_IN_SOURCE 的人**——
    // 只找 SYNCED 的話，第二次同步時他已經被標記過，就抓不到了，
    // 而 §7 規則 3 的「連續 N 次」需要每次都累計。
    //
    // PLATFORM_ONLY 的人排除在外：他們本來就不在來源裡。
    let missing: Vec<Uuid> = sqlx::query_scalar(
        "select id from app_user
         where sync_status in ('SYNCED', 'MISSING_IN_SOURCE')
           and employee_no is not null
           and employee_no <> all($1)",
    )
    .bind(&seen)
    .fetch_all(tx.executor())
    .await?;

    counts.missing_in_source_count = missing.len() as i32;

    for id in &missing {
        issues.push(SyncIssue {
            row_number: None,
            kind: "MISSING_IN_SOURCE",
            message: "此人在本次來源資料中消失。已標記，但未停用——\
                      來源缺漏與離職是兩回事"
                .into(),
            field: None,
            matched_ids: vec![*id],
        });
    }

    if !dry_run {
        if !missing.is_empty() {
            sqlx::query(
                "update app_user set sync_status = 'MISSING_IN_SOURCE', updated_at = now()
                 where id = any($1)",
            )
            .bind(&missing)
            .execute(tx.executor())
            .await?;

            record_pending_deactivation(tx, source_id, &missing).await?;
        }

        // 重新出現的人要從待停用清單移除。
        // 不清的話，他下次會因為一筆過期的紀錄而被建議停用
        sqlx::query(
            "delete from integration_pending_deactivation p
             using app_user u
             where p.user_id = u.id
               and p.source_id = $1
               and u.employee_no = any($2)",
        )
        .bind(source_id)
        .bind(&seen)
        .execute(tx.executor())
        .await?;
    }

    let status = if issues.iter().any(|i| i.kind == "VALIDATION_FAILED") {
        "PARTIAL_SUCCESS"
    } else {
        "SUCCESS"
    };

    Ok(SyncOutcome {
        status,
        counts,
        issues,
        message: None,
    })
}

/// 新建員工
///
/// `can_login = false`：同步進來的人多數不會登入（產線作業員、
/// 不用電腦的主管），但他們必須存在——可能是別人的主管、
/// 可能要被指派待辦（需求 §2）。
///
/// email 是 `not null`，沒有 email 時用員工編號組一個佔位值。
/// 組織健康檢查會把它報成「沒有 email」——那是對的，
/// 這個人確實收不到通知。
async fn insert_employee(
    tx: &mut TenantTx<'_>,
    _source_id: Uuid,
    row: &ValidEmployee,
    department_id: Option<Uuid>,
) -> Result<()> {
    let email = row.email.clone().unwrap_or_default();
    let name = row.name.clone().unwrap_or_else(|| row.employee_no.clone());

    sqlx::query(
        "insert into app_user
            (tenant_id, email, name, password_hash, department_id, employee_no,
             job_title, phone, extension, hired_at, left_at, external_id,
             source_system, sync_status, can_login)
         values ($1, $2, $3, null, $4, $5, $6, $7, $8, $9, $10, $11,
                 'FILE', 'SYNCED', false)",
    )
    .bind(tx.tenant_id())
    .bind(&email)
    .bind(&name)
    .bind(department_id)
    .bind(&row.employee_no)
    .bind(&row.job_title)
    .bind(&row.phone)
    .bind(&row.extension)
    .bind(row.hired_at)
    .bind(row.left_at)
    .bind(&row.external_id)
    .execute(tx.executor())
    .await
    .map_err(Error::from_db)?;

    Ok(())
}

/// 更新員工
///
/// 回傳 (有沒有改到東西, 因 ownership 跳過幾個欄位)。
///
/// `platform_managed_fields` 裡的欄位一律跳過並記一筆 issue——
/// 管理員要知道平台與 ERP 的差異在哪，而不只是一個總數。
async fn update_employee(
    tx: &mut TenantTx<'_>,
    current: &ExistingEmployee,
    row: &ValidEmployee,
    department_id: Option<Uuid>,
    dry_run: bool,
    issues: &mut Vec<SyncIssue>,
) -> Result<(bool, i32)> {
    // (欄位名, 值)。None 代表來源沒給這一欄，不動它
    let candidates: Vec<(&str, Option<String>)> = vec![
        ("name", row.name.clone()),
        ("email", row.email.clone()),
        ("job_title", row.job_title.clone()),
        ("phone", row.phone.clone()),
        ("extension", row.extension.clone()),
        ("external_id", row.external_id.clone()),
    ];

    let mut skipped = 0;
    let mut changed = false;

    for (field, value) in candidates {
        let Some(value) = value else { continue };

        if current.platform_managed_fields.iter().any(|f| f == field) {
            skipped += 1;
            issues.push(SyncIssue {
                row_number: Some(row.row_number),
                kind: "SKIPPED_BY_OWNERSHIP",
                message: format!("欄位「{field}」由平台維護，本次同步未覆蓋"),
                field: Some(field.to_string()),
                matched_ids: vec![current.id],
            });
            continue;
        }

        if !dry_run {
            let sql = format!("update app_user set {field} = $2 where id = $1");
            sqlx::query(&sql)
                .bind(current.id)
                .bind(&value)
                .execute(tx.executor())
                .await
                .map_err(Error::from_db)?;
        }
        changed = true;
    }

    // 日期與部門的型別不同，分開處理
    for (field, managed, has_value) in [
        ("department_id", "department_id", department_id.is_some()),
        ("hired_at", "hired_at", row.hired_at.is_some()),
        ("left_at", "left_at", row.left_at.is_some()),
    ] {
        if !has_value {
            continue;
        }
        if current.platform_managed_fields.iter().any(|f| f == managed) {
            skipped += 1;
            issues.push(SyncIssue {
                row_number: Some(row.row_number),
                kind: "SKIPPED_BY_OWNERSHIP",
                message: format!("欄位「{field}」由平台維護，本次同步未覆蓋"),
                field: Some(field.to_string()),
                matched_ids: vec![current.id],
            });
            continue;
        }

        if !dry_run {
            match field {
                "department_id" => {
                    sqlx::query("update app_user set department_id = $2 where id = $1")
                        .bind(current.id)
                        .bind(department_id)
                        .execute(tx.executor())
                        .await?;
                }
                "hired_at" => {
                    sqlx::query("update app_user set hired_at = $2 where id = $1")
                        .bind(current.id)
                        .bind(row.hired_at)
                        .execute(tx.executor())
                        .await?;
                }
                _ => {
                    sqlx::query("update app_user set left_at = $2 where id = $1")
                        .bind(current.id)
                        .bind(row.left_at)
                        .execute(tx.executor())
                        .await?;
                }
            }
        }
        changed = true;
    }

    // sync_status 無論有沒有改到欄位都要更新。
    //
    // 一個先前被標成 MISSING_IN_SOURCE 的人重新出現在來源裡時，
    // 他的資料可能一個字都沒變——若只在 changed 時更新，
    // 他會永遠卡在「來源消失」的狀態，而且待停用清單也清不掉。
    if !dry_run {
        sqlx::query(
            "update app_user set sync_status = 'SYNCED', last_synced_at = now(),
                                 updated_at = now()
             where id = $1",
        )
        .bind(current.id)
        .execute(tx.executor())
        .await?;
    }

    Ok((changed, skipped))
}

/// 記入待停用清單（§7 規則 3）
///
/// 連續消失 N 次才進入待管理員確認的階段。這裡只累計次數，
/// 判斷閾值與實際停用是另一支流程——**不自動停用**，
/// 那等於平台替客戶做人事決定。
async fn record_pending_deactivation(
    tx: &mut TenantTx<'_>,
    source_id: Uuid,
    missing: &[Uuid],
) -> Result<()> {
    sqlx::query(
        "insert into integration_pending_deactivation
            (tenant_id, source_id, user_id)
         select $1, $2, unnest($3::uuid[])
         on conflict (tenant_id, source_id, user_id) do update
            set miss_count = integration_pending_deactivation.miss_count + 1,
                last_missed_at = now()",
    )
    .bind(tx.tenant_id())
    .bind(source_id)
    .bind(missing)
    .execute(tx.executor())
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 完整性閘：首次匯入沒有比較基準
    #[test]
    fn 首次匯入不擋() {
        assert!(passes_completeness_gate(10, None, 0.9));
        assert!(passes_completeness_gate(0, None, 0.9));
    }

    #[test]
    fn 筆數足夠時放行() {
        // 1000 → 950，超過 90%
        assert!(passes_completeness_gate(950, Some(1000), 0.9));
        // 剛好在門檻上
        assert!(passes_completeness_gate(900, Some(1000), 0.9));
    }

    /// 這是整個同步最危險的判斷
    ///
    /// 來源 1000 人只回 823 人，若照寫，消失的 177 人會被當成離職。
    /// 其中只要有一個是主管，全公司相關流程立刻卡死
    #[test]
    fn 筆數驟減時中止() {
        assert!(!passes_completeness_gate(823, Some(1000), 0.9));
        assert!(!passes_completeness_gate(0, Some(1000), 0.9));
        assert!(!passes_completeness_gate(899, Some(1000), 0.9));
    }

    /// 上次是 0 筆時不擋——那通常代表上次也沒成功
    #[test]
    fn 上次為零時不擋() {
        assert!(passes_completeness_gate(5, Some(0), 0.9));
    }

    #[test]
    fn 門檻可調() {
        // 放寬到 50%
        assert!(passes_completeness_gate(500, Some(1000), 0.5));
        assert!(!passes_completeness_gate(499, Some(1000), 0.5));
    }

    #[test]
    fn 日期格式容錯() {
        let expected = NaiveDate::from_ymd_opt(2024, 3, 15).unwrap();

        assert_eq!(parse_date("2024-03-15"), Some(expected));
        assert_eq!(parse_date("2024/03/15"), Some(expected));
        assert_eq!(parse_date("20240315"), Some(expected));
        assert_eq!(parse_date("15/03/2024"), Some(expected));
    }

    #[test]
    fn 無法辨識的日期回_none() {
        assert_eq!(parse_date("民國113年3月15日"), None);
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("not a date"), None);
    }
}
