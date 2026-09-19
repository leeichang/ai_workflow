//! 組織資料同步
//!
//! 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §5～§8、§12.1。
//!
//! 本輪實作 P1（FILE connection：CSV 匯入）。
//!
//! ## 流程
//!
//! ```text
//! POST /integration/analyze   上傳 CSV，回表頭與欄位對應建議（§8）
//!        ↓  管理員逐欄確認
//! POST /integration/sources/{id}/preview   試跑，不寫入（§12.1「preview 是必備」）
//!        ↓  管理員看計數
//! POST /integration/sources/{id}/sync      正式寫入
//! ```
//!
//! ## 為什麼不用 multipart
//!
//! CSV 是文字，當 JSON 字串送即可。前端用 `FileReader.readAsText`
//! 讀出來直接放進 body，省掉 multipart 的 feature 與解析。
//! 檔案大小由 `domain::import::MAX_ROWS` 與 axum 的 body limit 擋。
//!
//! ## 權限
//!
//! 全部要 `admin`。同步會改動整份組織資料，而組織決定簽核路徑。

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/integration/analyze", post(analyze))
        .route(
            "/integration/connections",
            get(list_connections).post(create_connection),
        )
        .route(
            "/integration/sources",
            get(list_sources).post(create_source),
        )
        .route(
            "/integration/sources/{id}",
            get(get_source).patch(update_source).delete(delete_source),
        )
        .route("/integration/sources/{id}/preview", post(preview))
        .route("/integration/sources/{id}/sync", post(sync))
        .route("/integration/sync-runs", get(list_runs))
        .route("/integration/sync-runs/{id}/issues", get(list_issues))
        .route("/integration/pending-deactivations", get(list_pending))
        .route(
            "/integration/pending-deactivations/{id}/confirm",
            post(confirm_pending),
        )
        .route(
            "/integration/pending-deactivations/{id}/dismiss",
            post(dismiss_pending),
        )
}

// ── 分析檔案 ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AnalyzeBody {
    /// CSV 的完整內容
    pub content: String,
    /// employee 或 department
    pub dataset: String,
}

#[derive(Serialize)]
pub struct AnalyzeResponse {
    pub headers: Vec<String>,
    pub row_count: usize,
    /// 前幾列供管理員確認對應是否正確
    pub sample_rows: Vec<Vec<String>>,
    pub suggestions: Vec<domain::field_mapping::Suggestion>,
}

/// 上傳 CSV，回表頭與欄位對應建議
///
/// 不寫入任何東西，也不留下 sync_run——這只是「看看這份檔案長怎樣」。
///
/// sample_rows 只回前 5 列。回整份等於把全公司個資塞進一個
/// API 回應，而管理員確認欄位對應根本不需要那麼多。
async fn analyze(
    actor: Actor,
    Json(body): Json<AnalyzeBody>,
) -> ApiResult<Json<AnalyzeResponse>> {
    actor.require_any(&["admin"])?;

    let sheet = domain::import::parse_csv(body.content.as_bytes())
        .map_err(|e| ApiError::ValidationFailed(e.to_string()))?;

    let suggestions = domain::field_mapping::suggest(&sheet.headers, &body.dataset);

    const SAMPLE_SIZE: usize = 5;
    let sample_rows = sheet.rows.iter().take(SAMPLE_SIZE).cloned().collect();

    Ok(Json(AnalyzeResponse {
        headers: sheet.headers,
        row_count: sheet.rows.len(),
        sample_rows,
        suggestions,
    }))
}

// ── 連線 ────────────────────────────────────────────────

#[derive(Serialize, sqlx::FromRow)]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
}

async fn list_connections(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<Connection>>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    // 刻意不回 config 與 credential_ref：前者可能含內部主機名，
    // 後者是憑證的參照。清單畫面不需要它們
    let rows = sqlx::query_as::<_, Connection>(
        "select id, name, kind, enabled from integration_connection order by name",
    )
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢連線失敗：{e}")))?;

    tx.commit().await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct NewConnection {
    pub name: String,
    pub kind: String,
}

async fn create_connection(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<NewConnection>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    if body.kind != "FILE" {
        return Err(ApiError::ValidationFailed(
            "目前只支援 FILE 連線。REST 與 SQL 尚未實作".into(),
        ));
    }
    if body.name.trim().is_empty() {
        return Err(ApiError::ValidationFailed("名稱不可空白".into()));
    }

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let id: Uuid = sqlx::query_scalar(
        "insert into integration_connection (tenant_id, name, kind, created_by)
         values ($1, $2, $3, $4) returning id",
    )
    .bind(actor.tenant_id)
    .bind(body.name.trim())
    .bind(&body.kind)
    .bind(actor.user_id)
    .fetch_one(tx.executor())
    .await
    .map_err(|e| ApiError::from(persistence::Error::from_db(e)))?;

    tx.audit(
        persistence::AuditEvent::new("internal", "integration.connection.create", "integration_connection")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "name": body.name, "kind": body.kind })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

// ── 來源 ────────────────────────────────────────────────

#[derive(Serialize, sqlx::FromRow)]
pub struct Source {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub dataset: String,
    pub name: String,
    pub mapping_definition: serde_json::Value,
    pub completeness_threshold: f64,
    /// 完整性閘的絕對值條件。減少人數低於此值時不中止（Q-02）
    pub min_drop_threshold: i32,
    /// 連續消失幾次才進待停用清單（Q-03）
    pub deactivation_miss_threshold: i32,
    pub last_success_count: Option<i32>,
    pub last_synced_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub struct NewSource {
    pub connection_id: Uuid,
    pub dataset: String,
    pub name: String,
    /// 來源欄位名 → canonical 欄位名
    pub mapping: HashMap<String, String>,
}

/// 建立來源
///
/// mapping 在這裡固化。§8 的設計是「AI 建議 → 人工確認 → 固化」，
/// 確認後就不再問 AI——每次重問會讓結果漂移，而 mapping 一旦錯了
/// 就是整批資料寫錯欄位。
async fn create_source(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<NewSource>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;
    validate_mapping(&body.dataset, &body.mapping)?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let id: Uuid = sqlx::query_scalar(
        "insert into integration_source
            (tenant_id, connection_id, dataset, name, mapping_definition)
         values ($1, $2, $3, $4, $5) returning id",
    )
    .bind(actor.tenant_id)
    .bind(body.connection_id)
    .bind(&body.dataset)
    .bind(&body.name)
    .bind(serde_json::to_value(&body.mapping).unwrap_or_default())
    .fetch_one(tx.executor())
    .await
    .map_err(|e| ApiError::from(persistence::Error::from_db(e)))?;

    tx.audit(
        persistence::AuditEvent::new("internal", "integration.source.create", "integration_source")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            // 記欄位對應而非資料。日後查「為什麼這欄同步錯了」要看得到
            .payload(json!({ "dataset": body.dataset, "mapping": body.mapping })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

/// 列出來源
///
/// 匯入精靈要靠它重用既有的來源。每次上傳都建新的話，
/// `unique (tenant_id, connection_id, dataset)` 會擋下第二次；
/// 就算擋得過，新來源沒有 `last_success_count`，
/// **完整性閘會永遠不生效**——那是最危險的失效方式，
/// 因為它安靜。
async fn list_sources(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<Source>>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let rows = sqlx::query_as::<_, Source>(
        "select id, connection_id, dataset, name, mapping_definition,
                completeness_threshold::float8 as completeness_threshold,
                min_drop_threshold, deactivation_miss_threshold,
                last_success_count, last_synced_at
         from integration_source order by name",
    )
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢來源失敗：{e}")))?;

    tx.commit().await?;
    Ok(Json(rows))
}

async fn get_source(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Source>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let row = sqlx::query_as::<_, Source>(
        "select id, connection_id, dataset, name, mapping_definition,
                completeness_threshold::float8 as completeness_threshold,
                min_drop_threshold, deactivation_miss_threshold,
                last_success_count, last_synced_at
         from integration_source where id = $1",
    )
    .bind(id)
    .fetch_optional(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢來源失敗：{e}")))?
    .ok_or_else(|| ApiError::NotFound {
        kind: "integration_source".into(),
        id: id.to_string(),
    })?;

    tx.commit().await?;
    Ok(Json(row))
}

/// 刪除來源
///
/// 換了人事系統、欄位對應完全不同時，重新設定比逐欄改容易。
///
/// 同步歷史（`integration_sync_run`）是 cascade，會跟著刪掉。
/// 那是刻意的：歷史的意義是「這個來源同步過什麼」，來源沒了
/// 之後那些計數對不上任何東西。稽核事件留在 `audit_event`，
/// 不受影響。
async fn delete_source(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let affected = sqlx::query("delete from integration_source where id = $1")
        .bind(id)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("刪除來源失敗：{e}")))?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound {
            kind: "integration_source".into(),
            id: id.to_string(),
        });
    }

    tx.audit(
        persistence::AuditEvent::new("internal", "integration.source.delete", "integration_source")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string()),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SourcePatch {
    pub mapping: Option<HashMap<String, String>>,
    pub completeness_threshold: Option<f64>,
    /// 完整性閘的絕對值條件（Q-02）
    pub min_drop_threshold: Option<i32>,
    /// 連續消失幾次才進待停用清單（Q-03）
    pub deactivation_miss_threshold: Option<i32>,
}

async fn update_source(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<SourcePatch>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    if let Some(mapping) = &body.mapping {
        let dataset: String =
            sqlx::query_scalar("select dataset from integration_source where id = $1")
                .bind(id)
                .fetch_optional(tx.executor())
                .await
                .map_err(|e| ApiError::Internal(format!("查詢來源失敗：{e}")))?
                .ok_or_else(|| ApiError::NotFound {
                    kind: "integration_source".into(),
                    id: id.to_string(),
                })?;

        validate_mapping(&dataset, mapping)?;

        sqlx::query(
            "update integration_source set mapping_definition = $2, updated_at = now()
             where id = $1",
        )
        .bind(id)
        .bind(serde_json::to_value(mapping).unwrap_or_default())
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("更新對應失敗：{e}")))?;
    }

    if let Some(threshold) = body.completeness_threshold {
        // 0 等於關掉完整性閘。要擋——那是整個同步最重要的保護
        if threshold <= 0.0 || threshold > 1.0 {
            return Err(ApiError::ValidationFailed(
                "完整性門檻必須大於 0 且不超過 1。設為 0 等於關掉保護".into(),
            ));
        }

        sqlx::query(
            "update integration_source set completeness_threshold = $2, updated_at = now()
             where id = $1",
        )
        .bind(id)
        .bind(threshold)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("更新門檻失敗：{e}")))?;
    }

    if let Some(value) = body.min_drop_threshold {
        // 設成 0 等於「只要比例不過就擋」，那會讓小公司天天誤觸發。
        // 但那是管理員的選擇，只要不是負數就放行
        if value < 1 {
            return Err(ApiError::ValidationFailed(
                "絕對值門檻至少為 1".into(),
            ));
        }
        sqlx::query(
            "update integration_source set min_drop_threshold = $2, updated_at = now()
             where id = $1",
        )
        .bind(id)
        .bind(value)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("更新門檻失敗：{e}")))?;
    }

    if let Some(value) = body.deactivation_miss_threshold {
        // 設成 0 等於「消失一次就可以停用」，那會讓一次匯出疏漏
        // 就把人停掉——§7 規則 3 的整個用意就是避免這件事
        if value < 1 {
            return Err(ApiError::ValidationFailed(
                "待停用門檻至少為 1。設為 0 等於一次匯出疏漏就能停用".into(),
            ));
        }
        sqlx::query(
            "update integration_source
             set deactivation_miss_threshold = $2, updated_at = now()
             where id = $1",
        )
        .bind(id)
        .bind(value)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("更新門檻失敗：{e}")))?;
    }

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// 檢查 mapping 只用得到認得的 canonical 欄位
///
/// 寫死的白名單。mapping 會存進資料庫並長期沿用，允許任意字串
/// 會讓同步時讀到不認得的欄位名——那時只能靜默忽略，
/// 而管理員會以為資料有同步。
fn validate_mapping(dataset: &str, mapping: &HashMap<String, String>) -> Result<(), ApiError> {
    use domain::field_mapping::{DEPARTMENT_FIELDS, EMPLOYEE_FIELDS};

    let allowed: Vec<&str> = match dataset {
        "employee" => EMPLOYEE_FIELDS.iter().map(|f| f.as_str()).collect(),
        "department" => DEPARTMENT_FIELDS.iter().map(|f| f.as_str()).collect(),
        other => {
            return Err(ApiError::ValidationFailed(format!(
                "未知的資料集「{other}」。可用的是 employee 或 department"
            )))
        }
    };

    for target in mapping.values() {
        if !allowed.contains(&target.as_str()) {
            return Err(ApiError::ValidationFailed(format!(
                "「{target}」不是 {dataset} 的可對應欄位。可用的欄位：{}",
                allowed.join("、")
            )));
        }
    }

    // employee_no 是匹配鍵。沒有它，同步無法判斷是新人還是既有員工，
    // 每次都會新建——同步兩次就有兩份
    if dataset == "employee" && !mapping.values().any(|v| v == "employee_no") {
        return Err(ApiError::ValidationFailed(
            "必須對應員工編號。沒有它無法判斷是新人還是既有員工，同步兩次會產生兩份資料"
                .into(),
        ));
    }
    if dataset == "department" && !mapping.values().any(|v| v == "code") {
        return Err(ApiError::ValidationFailed("必須對應部門代碼".into()));
    }

    Ok(())
}

// ── 同步 ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SyncBody {
    /// CSV 的完整內容
    pub content: String,
}

/// 試跑
///
/// §12.1：「preview 是必備，不是加分項。沒有試跑，管理員第一次同步
/// 就是盲賭。」
///
/// 完全不寫入，但計數照算。管理員要能先看到「會新增 12 筆、
/// 更新 340 筆、跳過 8 個欄位」。
async fn preview(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<SyncBody>,
) -> ApiResult<Json<persistence::sync::SyncOutcome>> {
    run_sync(state, actor, id, body, true).await
}

/// 正式同步
async fn sync(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<SyncBody>,
) -> ApiResult<Json<persistence::sync::SyncOutcome>> {
    run_sync(state, actor, id, body, false).await
}

/// preview 與正式同步共用
///
/// 差別只有 `dry_run`。共用一條路徑是刻意的——若兩邊各寫一次，
/// preview 顯示的結果與實際寫入的結果就可能不同，
/// 那會讓 preview 失去意義。
async fn run_sync(
    state: AppState,
    actor: Actor,
    source_id: Uuid,
    body: SyncBody,
    dry_run: bool,
) -> ApiResult<Json<persistence::sync::SyncOutcome>> {
    actor.require_any(&["admin"])?;

    let sheet = domain::import::parse_csv(body.content.as_bytes())
        .map_err(|e| ApiError::ValidationFailed(e.to_string()))?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let source = sqlx::query_as::<_, (String, serde_json::Value, f64, i32, Option<i32>)>(
        "select dataset, mapping_definition,
                completeness_threshold::float8, min_drop_threshold,
                last_success_count
         from integration_source where id = $1",
    )
    .bind(source_id)
    .fetch_optional(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢來源失敗：{e}")))?
    .ok_or_else(|| ApiError::NotFound {
        kind: "integration_source".into(),
        id: source_id.to_string(),
    })?;

    let (dataset, mapping_value, threshold, min_drop, last_success_count) = source;

    if dataset != "employee" {
        return Err(ApiError::ValidationFailed(
            "目前只支援員工資料的同步。部門請用組織管理手動維護".into(),
        ));
    }

    let mapping: HashMap<String, String> =
        serde_json::from_value(mapping_value).map_err(|e| {
            ApiError::Internal(format!("欄位對應的格式損壞：{e}"))
        })?;

    // domain 的 MappedRow 轉成 persistence 的 SyncRow。
    // 兩層刻意不共用型別，轉換在這裡做——這裡本來就同時看得到兩邊
    let rows: Vec<persistence::sync::SyncRow> = domain::import::map_rows(&sheet, &mapping)
        .into_iter()
        .map(|r| persistence::sync::SyncRow {
            row_number: r.row_number,
            values: r.values,
        })
        .collect();

    let outcome = persistence::sync::sync_employees(
        &mut tx,
        source_id,
        &rows,
        last_success_count,
        threshold,
        min_drop,
        dry_run,
    )
    .await?;

    // 試跑也留一筆 run。管理員回頭要看得到「我那天試跑的結果是什麼」
    record_run(&mut tx, source_id, actor.user_id, dry_run, &outcome).await?;

    // 正式同步成功才更新比較基準。試跑或中止都不更新——
    // 用一次不完整的同步當基準，下一次的完整性閘就失效了
    if !dry_run && outcome.status != "ABORTED_INCOMPLETE" {
        sqlx::query(
            "update integration_source
             set last_success_count = $2, last_synced_at = now(), updated_at = now()
             where id = $1",
        )
        .bind(source_id)
        .bind(outcome.counts.validated_count)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("更新來源狀態失敗：{e}")))?;
    }

    tx.audit(
        persistence::AuditEvent::new(
            "internal",
            if dry_run {
                "integration.preview"
            } else {
                "integration.sync"
            },
            "integration_source",
        )
        .actor(actor.user_id.to_string(), actor.name.clone())
        .target(source_id.to_string())
        // 記計數而非資料。稽核紀錄不該變成另一份個資副本
        .payload(json!({
            "status": outcome.status,
            "counts": outcome.counts,
        })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(outcome))
}

/// 寫入 sync_run 與問題明細
async fn record_run(
    tx: &mut persistence::TenantTx<'_>,
    source_id: Uuid,
    actor_id: Uuid,
    dry_run: bool,
    outcome: &persistence::sync::SyncOutcome,
) -> ApiResult<()> {
    let c = &outcome.counts;

    let run_id: Uuid = sqlx::query_scalar(
        "insert into integration_sync_run
            (tenant_id, source_id, status, dry_run, started_by, finished_at,
             received_count, parsed_count, mapped_count, validated_count,
             inserted_count, updated_count, unchanged_count,
             skipped_by_ownership_count, rejected_count, missing_in_source_count,
             message)
         values ($1, $2, $3, $4, $5, now(),
                 $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         returning id",
    )
    .bind(tx.tenant_id())
    .bind(source_id)
    .bind(outcome.status)
    .bind(dry_run)
    .bind(actor_id)
    .bind(c.received_count)
    .bind(c.parsed_count)
    .bind(c.mapped_count)
    .bind(c.validated_count)
    .bind(c.inserted_count)
    .bind(c.updated_count)
    .bind(c.unchanged_count)
    .bind(c.skipped_by_ownership_count)
    .bind(c.rejected_count)
    .bind(c.missing_in_source_count)
    .bind(&outcome.message)
    .fetch_one(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("寫入同步紀錄失敗：{e}")))?;

    // 問題明細。數量可能很多，一次寫入而非逐筆
    for issue in &outcome.issues {
        sqlx::query(
            "insert into integration_sync_issue
                (tenant_id, run_id, row_number, kind, message, field, matched_ids)
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(tx.tenant_id())
        .bind(run_id)
        .bind(issue.row_number)
        .bind(issue.kind)
        .bind(&issue.message)
        .bind(&issue.field)
        .bind(&issue.matched_ids)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("寫入問題明細失敗：{e}")))?;
    }

    Ok(())
}

// ── 歷史 ────────────────────────────────────────────────

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncRun {
    pub id: Uuid,
    pub source_id: Uuid,
    pub status: String,
    pub dry_run: bool,
    pub received_count: i32,
    pub validated_count: i32,
    pub inserted_count: i32,
    pub updated_count: i32,
    pub unchanged_count: i32,
    pub skipped_by_ownership_count: i32,
    pub rejected_count: i32,
    pub missing_in_source_count: i32,
    pub message: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn list_runs(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<SyncRun>>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let rows = sqlx::query_as::<_, SyncRun>(
        "select id, source_id, status, dry_run,
                received_count, validated_count, inserted_count, updated_count,
                unchanged_count, skipped_by_ownership_count, rejected_count,
                missing_in_source_count, message, started_at, finished_at
         from integration_sync_run
         order by started_at desc
         limit 50",
    )
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢同步紀錄失敗：{e}")))?;

    tx.commit().await?;
    Ok(Json(rows))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncIssueRow {
    pub id: Uuid,
    pub row_number: Option<i32>,
    pub kind: String,
    pub message: String,
    pub field: Option<String>,
}

// ── 待停用（§7 規則 3、4）──────────────────────────────

/// 待停用清單
///
/// 列出所有 `MISSING_IN_SOURCE` 的人，含連續消失次數與門檻。
/// **未達門檻的也列出來**——管理員要看得到「這個人消失了，
/// 但還不到該處理的程度」，而不是等它突然冒出來。
///
/// `blockers` 是停用前置檢查的結果，每次都即時算。組織會變動，
/// 一小時前算的結果現在可能已經不成立。
async fn list_pending(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<persistence::sync::PendingDeactivation>>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let rows = persistence::sync::list_pending_deactivations(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

/// 確認停用
///
/// §7 規則 3 說「由管理員確認後才改 status」。前置檢查不過時拒絕——
/// 停用一個主管會讓他所有部屬送單時找不到簽核人。
async fn confirm_pending(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let user_id = persistence::sync::confirm_deactivation(&mut tx, id, actor.user_id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "integration.deactivation.confirm", "app_user")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(user_id.to_string()),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// 忽略
///
/// 管理員判斷這個人仍在職，只是匯出漏了。要稽核——
/// 日後這個人真的離職卻還在收任務時，要查得到是誰判斷他還在。
async fn dismiss_pending(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let user_id = persistence::sync::dismiss_deactivation(&mut tx, id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "integration.deactivation.dismiss", "app_user")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(user_id.to_string()),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_issues(
    State(state): State<AppState>,
    actor: Actor,
    Path(run_id): Path<Uuid>,
) -> ApiResult<Json<Vec<SyncIssueRow>>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let rows = sqlx::query_as::<_, SyncIssueRow>(
        "select id, row_number, kind, message, field
         from integration_sync_issue
         where run_id = $1
         order by row_number nulls last, id
         limit 500",
    )
    .bind(run_id)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢問題明細失敗：{e}")))?;

    tx.commit().await?;
    Ok(Json(rows))
}
