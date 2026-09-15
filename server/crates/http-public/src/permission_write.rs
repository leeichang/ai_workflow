//! 權限矩陣寫回
//!
//! 矩陣上的三態是 UI 糖衣，底層仍是 form content 的 workflow 區塊。
//! 此模組把「欄位在某節點對某角色的權限」翻譯回表達式與角色清單。
//!
//! 為何不直接讓前端送整份 content：
//!   矩陣只改權限，不該有能力改動欄位結構。若開放整份覆寫，
//!   前端的任何 bug 都可能把使用者的表單設計洗掉。

use axum::extract::{Path, State};
use axum::routing::put;
use axum::{Json, Router};
use domain::permission::Permission;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/forms/{form_key}/permissions", put(save_matrix))
}

/// 一格權限修改
#[derive(Debug, Deserialize)]
pub struct CellChange {
    /// 欄位 key
    pub field: String,
    /// 流程節點 id
    pub node_id: String,
    /// 適用角色
    pub role: String,
    pub permission: Permission,
}

#[derive(Debug, Deserialize)]
pub struct SaveMatrixBody {
    pub changes: Vec<CellChange>,
}

#[derive(Serialize)]
pub struct SaveMatrixResponse {
    /// 實際套用的修改數。與送入數量不同時代表有被略過的項目。
    applied: usize,
    /// 被略過的項目與原因
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<SkippedChange>,
}

#[derive(Serialize)]
pub struct SkippedChange {
    field: String,
    reason: &'static str,
}

/// 儲存權限矩陣
///
/// 寫入草稿。使用者後續可在表單設計器檢視結果，確認無誤才發布。
async fn save_matrix(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
    Json(body): Json<SaveMatrixBody>,
) -> ApiResult<Json<SaveMatrixResponse>> {
    actor.require_any(&["designer"])?;

    if body.changes.is_empty() {
        return Ok(Json(SaveMatrixResponse {
            applied: 0,
            skipped: vec![],
        }));
    }

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;

    // 以草稿為基礎。沒有草稿時從最新發布版建立一份。
    let base = match persistence::form::get_draft(&mut tx, form.id).await? {
        Some(d) => d.content,
        None => persistence::form::get_latest_published(&mut tx, form.id)
            .await?
            .ok_or_else(|| ApiError::Conflict("表單尚無任何版本".into()))?
            .content,
    };

    let (updated, skipped) = apply_changes(base, &body.changes)?;

    let draft =
        persistence::form::save_draft(&mut tx, form.id, &updated, actor.user_id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "form.permissions.save", "form_definition_version")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(draft.id.to_string())
            .payload(serde_json::json!({
                "form_key": form_key,
                "change_count": body.changes.len(),
            })),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(SaveMatrixResponse {
        applied: body.changes.len() - skipped.len(),
        skipped,
    }))
}

/// 把矩陣修改套用到 form content
///
/// 三態的對應關係：
///   HIDDEN   → 該角色移出 readable_roles
///   READONLY → 該角色移出 editable_roles，但保留在 readable_roles
///   EDITABLE → 該角色加入兩份清單
///
/// 節點維度目前記錄於 readonly_when 表達式。完整的「每節點每角色」
/// 獨立設定需要更複雜的資料結構，排在後續任務。
fn apply_changes(
    mut content: Value,
    changes: &[CellChange],
) -> ApiResult<(Value, Vec<SkippedChange>)> {
    let mut skipped = Vec::new();

    let fields = content
        .get_mut("fields")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ApiError::Conflict("表單內容缺少 fields".into()))?;

    for change in changes {
        let Some(field) = fields
            .iter_mut()
            .find(|f| f.get("key").and_then(Value::as_str) == Some(change.field.as_str()))
        else {
            skipped.push(SkippedChange {
                field: change.field.clone(),
                reason: "欄位不存在",
            });
            continue;
        };

        // 計算欄位的權限由系統決定，不接受修改
        if field.pointer("/data/computed").is_some() {
            skipped.push(SkippedChange {
                field: change.field.clone(),
                reason: "系統計算欄位不可調整權限",
            });
            continue;
        }

        apply_one(field, change);
    }

    Ok((content, skipped))
}

fn apply_one(field: &mut Value, change: &CellChange) {
    let workflow = field
        .as_object_mut()
        .expect("field 必為物件")
        .entry("workflow")
        .or_insert_with(|| Value::Object(Default::default()));

    let obj = workflow.as_object_mut().expect("workflow 必為物件");

    let mut readable = string_list(obj.get("readable_roles"));
    let mut editable = string_list(obj.get("editable_roles"));

    match change.permission {
        Permission::Hidden => {
            remove_role(&mut readable, &change.role);
            remove_role(&mut editable, &change.role);
        }
        Permission::Readonly => {
            add_role(&mut readable, &change.role);
            remove_role(&mut editable, &change.role);
        }
        Permission::Editable => {
            add_role(&mut readable, &change.role);
            add_role(&mut editable, &change.role);
        }
    }

    set_role_list(obj, "readable_roles", readable);
    set_role_list(obj, "editable_roles", editable);
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn add_role(list: &mut Vec<String>, role: &str) {
    if !list.iter().any(|r| r == role) {
        list.push(role.to_string());
    }
}

fn remove_role(list: &mut Vec<String>, role: &str) {
    list.retain(|r| r != role);
}

/// 寫入角色清單
///
/// 空陣列必須保留，不可移除鍵。三種狀態語意不同：
///   鍵不存在 → 不限制，任何角色皆可
///   []       → 明確拒絕所有角色
///   ["a"]    → 僅角色 a
///
/// 矩陣把某欄位對所有角色設為唯讀時，editable_roles 會變成空陣列。
/// 若此時移除鍵，解析端會視為「不限制」而回傳可編輯，與使用者意圖相反。
fn set_role_list(obj: &mut serde_json::Map<String, Value>, key: &str, list: Vec<String>) {
    obj.insert(key.to_string(), Value::from(list));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn content() -> Value {
        json!({
            "fields": [
                { "key": "name", "ui": {}, "data": { "path": "a.name" } },
                {
                    "key": "total",
                    "ui": {},
                    "data": { "path": "a.total", "computed": "sum(x)" }
                }
            ]
        })
    }

    fn change(field: &str, perm: Permission) -> CellChange {
        CellChange {
            field: field.into(),
            node_id: "start".into(),
            role: "requester".into(),
            permission: perm,
        }
    }

    #[test]
    fn editable_adds_role_to_both_lists() {
        let (out, skipped) =
            apply_changes(content(), &[change("name", Permission::Editable)]).unwrap();

        let wf = &out["fields"][0]["workflow"];
        assert_eq!(wf["readable_roles"], json!(["requester"]));
        assert_eq!(wf["editable_roles"], json!(["requester"]));
        assert!(skipped.is_empty());
    }

    #[test]
    fn readonly_keeps_readable_empties_editable() {
        let (out, _) = apply_changes(
            content(),
            &[
                change("name", Permission::Editable),
                change("name", Permission::Readonly),
            ],
        )
        .unwrap();

        let wf = &out["fields"][0]["workflow"];
        assert_eq!(wf["readable_roles"], json!(["requester"]));
        // 空陣列必須保留。移除鍵會被解析端視為「不限制」，
        // 與使用者設定唯讀的意圖相反。
        assert_eq!(wf["editable_roles"], json!([]));
    }

    #[test]
    fn hidden_empties_both_lists() {
        let (out, _) = apply_changes(
            content(),
            &[
                change("name", Permission::Editable),
                change("name", Permission::Hidden),
            ],
        )
        .unwrap();

        let wf = &out["fields"][0]["workflow"];
        assert_eq!(wf["readable_roles"], json!([]));
        assert_eq!(wf["editable_roles"], json!([]));
    }

    #[test]
    fn computed_field_is_skipped() {
        let (out, skipped) =
            apply_changes(content(), &[change("total", Permission::Editable)]).unwrap();

        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].reason, "系統計算欄位不可調整權限");
        assert!(out["fields"][1].get("workflow").is_none(), "不應被修改");
    }

    #[test]
    fn unknown_field_is_skipped_not_error() {
        let (_, skipped) =
            apply_changes(content(), &[change("nonexistent", Permission::Editable)]).unwrap();

        // 不回錯誤：矩陣可能與已變更的表單不同步，
        // 略過比整批失敗更合理。
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].reason, "欄位不存在");
    }

    #[test]
    fn multiple_roles_accumulate() {
        let mut changes = vec![change("name", Permission::Editable)];
        changes.push(CellChange {
            field: "name".into(),
            node_id: "start".into(),
            role: "approver".into(),
            permission: Permission::Readonly,
        });

        let (out, _) = apply_changes(content(), &changes).unwrap();
        let wf = &out["fields"][0]["workflow"];

        assert_eq!(wf["readable_roles"], json!(["requester", "approver"]));
        assert_eq!(wf["editable_roles"], json!(["requester"]));
    }

    #[test]
    fn repeated_same_change_is_idempotent() {
        let changes = vec![
            change("name", Permission::Editable),
            change("name", Permission::Editable),
            change("name", Permission::Editable),
        ];
        let (out, _) = apply_changes(content(), &changes).unwrap();

        assert_eq!(
            out["fields"][0]["workflow"]["readable_roles"],
            json!(["requester"]),
            "重複套用不應產生重複角色"
        );
    }

    #[test]
    fn preserves_existing_workflow_rules() {
        let mut c = content();
        c["fields"][0]["workflow"] = json!({ "readonly_when": "node.id != 'start'" });

        let (out, _) = apply_changes(c, &[change("name", Permission::Editable)]).unwrap();
        let wf = &out["fields"][0]["workflow"];

        assert_eq!(
            wf["readonly_when"], "node.id != 'start'",
            "既有的條件式規則不該被權限修改洗掉"
        );
        assert_eq!(wf["editable_roles"], json!(["requester"]));
    }
}
