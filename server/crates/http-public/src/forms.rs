//! 表單定義 API
//!
//! 端點對應 docs/系統規劃/03_功能規劃.md 4.4 節。
//!
//! 權限：
//!   讀取需登入即可，因為流程執行時所有參與者都要取得表單。
//!   修改與發布需 designer 或 admin。

use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/forms", get(list).post(create))
        .route("/forms/{form_key}", get(get_one))
        .route("/forms/{form_key}/draft", put(save_draft))
        .route("/forms/{form_key}/draft", delete(discard_draft))
        .route("/forms/{form_key}/draft/validate", post(validate_draft))
        .route("/forms/{form_key}/draft/publish", post(publish))
        .route("/forms/{form_key}/versions", get(list_versions))
        .route("/forms/{form_key}/versions/{version}", get(get_version))
}

// ── 回應型別 ────────────────────────────────────────────

#[derive(Serialize)]
pub struct FormDetail {
    #[serde(flatten)]
    definition: persistence::form::FormDefinition,
    /// 當前草稿，沒有時為 null
    draft: Option<persistence::form::FormVersion>,
    /// 最新已發布版本，尚未發布過時為 null
    published: Option<persistence::form::FormVersion>,
}

#[derive(Serialize)]
pub struct ValidationResult {
    valid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
    /// 不影響 `valid` 的提醒。見 `business_object_warnings`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

/// 發布成功的回應
///
/// 比 `FormVersion` 多一個 warnings。發布可能帶著警告成功，
/// 呼叫端要能拿到那些警告顯示給使用者。
#[derive(Serialize)]
pub struct PublishResult {
    #[serde(flatten)]
    version: persistence::form::FormVersion,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

/// 表單的業務物件相關警告
///
/// 表單不像流程會引用路徑，所以沒有 WF-E012 那種靜默跳過簽核的風險。
/// 但它有另一個：**欄位的 `data.path` 決定日後流程能引用什麼**。
///
/// 路徑寫成 `quotation.total_amount`（已淘汰的舊名稱）時，表單本身能用，
/// 但流程的條件式 `quotation.total > x` 就取不到值——而這要等到流程
/// 跑起來才會發現，那時已經太晚。這正是 seed.sh 與 fixture 曾有兩套
/// 詞彙的問題來源。
///
/// 兩種警告都不擋下發布：
///   - 業務物件未定義時仍應可用，否則使用者要先請人幫他建 JSON 定義
///   - 路徑不在定義裡可能是刻意的（純顯示欄位、暫存欄位）
fn business_object_warnings(business_object: &str, content: &Value) -> Vec<String> {
    if !domain::business_object::is_known(business_object) {
        let known = domain::business_object::known_business_objects().join("、");
        return vec![format!(
            "業務物件「{business_object}」尚未定義，\
             日後為此表單建立流程時，條件式與簽核人路徑不會被檢查。\
             已定義的業務物件：{known}"
        )];
    }

    let known_paths = domain::business_object::paths_for(business_object);
    let mut unknown: Vec<String> = content
        .get("fields")
        .and_then(Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .filter_map(|f| f.get("data")?.get("path")?.as_str())
                // 只檢查屬於這個業務物件的路徑。表單可以引用其他物件的
                // 資料（例如 customer.name），那不在本業務物件的定義裡
                // 是正常的。
                .filter(|p| p.starts_with(&format!("{business_object}.")))
                .filter(|p| !known_paths.contains(*p))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    if unknown.is_empty() {
        return Vec::new();
    }

    unknown.sort();
    unknown.dedup();
    vec![format!(
        "以下欄位路徑不在「{business_object}」的正式定義中：{}。\
         流程的條件式引用正式路徑時會取不到這些欄位的值。",
        unknown.join("、")
    )]
}

#[derive(Deserialize)]
pub struct SaveDraftBody {
    content: Value,
}

// ── Handler ─────────────────────────────────────────────

async fn list(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<persistence::form::FormSummary>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let forms = persistence::form::list(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(forms))
}

async fn get_one(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
) -> ApiResult<Json<FormDetail>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let definition = persistence::form::find_by_key(&mut tx, &form_key).await?;
    let draft = persistence::form::get_draft(&mut tx, definition.id).await?;
    let published = persistence::form::get_latest_published(&mut tx, definition.id).await?;

    tx.commit().await?;
    Ok(Json(FormDetail {
        definition,
        draft,
        published,
    }))
}

async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<persistence::form::CreateForm>,
) -> ApiResult<(axum::http::StatusCode, Json<FormDetail>)> {
    actor.require_any(&["designer"])?;

    // 建立時只驗結構，不驗語意。使用者可能先建空殼再慢慢填。
    domain::form_schema::validate(&input.content)?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form_key = input.form_key.clone();

    let (definition, draft) = persistence::form::create(&mut tx, input, actor.user_id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "form.create", "form_definition")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(definition.id.to_string())
            .payload(serde_json::json!({ "form_key": form_key })),
    )
    .await?;

    tx.commit().await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(FormDetail {
            definition,
            draft: Some(draft),
            published: None,
        }),
    ))
}

/// 儲存草稿
///
/// 刻意不在此驗證語意。使用者編輯到一半的內容常常是不完整的，
/// 每次存檔都報錯會很煩。語意檢查留到發布時。
async fn save_draft(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
    Json(body): Json<SaveDraftBody>,
) -> ApiResult<Json<persistence::form::FormVersion>> {
    actor.require_any(&["designer"])?;
    domain::form_schema::validate(&body.content)?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;
    let draft = persistence::form::save_draft(&mut tx, form.id, &body.content, actor.user_id).await?;

    tx.commit().await?;
    Ok(Json(draft))
}

async fn discard_draft(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;
    let removed = persistence::form::discard_draft(&mut tx, form.id).await?;

    if removed {
        tx.audit(
            persistence::AuditEvent::new("internal", "form.discard_draft", "form_definition")
                .actor(actor.user_id.to_string(), actor.name.clone())
                .target(form.id.to_string()),
        )
        .await?;
    }

    tx.commit().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// 驗證草稿但不發布
///
/// 讓前端可在使用者按發布前先提示問題，避免按下去才失敗。
async fn validate_draft(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
) -> ApiResult<Json<ValidationResult>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;
    let draft = persistence::form::get_draft(&mut tx, form.id).await?;
    tx.commit().await?;

    let Some(draft) = draft else {
        return Err(ApiError::Conflict("沒有草稿可驗證".into()));
    };

    let warnings = business_object_warnings(&form.business_object, &draft.content);

    match domain::form_schema::validate_for_publish(&draft.content) {
        Ok(()) => Ok(Json(ValidationResult {
            valid: true,
            errors: vec![],
            warnings,
        })),
        Err(e) => Ok(Json(ValidationResult {
            valid: false,
            errors: e.to_string().split('；').map(str::to_owned).collect(),
            warnings,
        })),
    }
}

/// 發布草稿為新版本
///
/// 此處做完整驗證。發布代表這份定義要被流程實際使用，
/// 不能讓壞資料進入正式版本。
async fn publish(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
) -> ApiResult<Json<PublishResult>> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;

    let draft = persistence::form::get_draft(&mut tx, form.id)
        .await?
        .ok_or_else(|| ApiError::Conflict("沒有可發布的草稿".into()))?;

    domain::form_schema::validate_for_publish(&draft.content)?;

    let warnings = business_object_warnings(&form.business_object, &draft.content);
    let published = persistence::form::publish(&mut tx, form.id, actor.user_id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "form.publish", "form_definition_version")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(published.id.to_string())
            .payload(serde_json::json!({
                "form_key": form_key,
                "version": published.version,
                // 警告也要記——事後查得到哪些表單的路徑偏離了正式定義
                "warnings": warnings,
            })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(PublishResult {
        version: published,
        warnings,
    }))
}

async fn list_versions(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
) -> ApiResult<Json<Vec<persistence::form::FormVersion>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;
    let versions = persistence::form::list_versions(&mut tx, form.id).await?;
    tx.commit().await?;
    Ok(Json(versions))
}

async fn get_version(
    State(state): State<AppState>,
    actor: Actor,
    Path((form_key, version)): Path<(String, i32)>,
) -> ApiResult<Json<persistence::form::FormVersion>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;
    let v = persistence::form::get_version(&mut tx, form.id, version).await?;
    tx.commit().await?;
    Ok(Json(v))
}
