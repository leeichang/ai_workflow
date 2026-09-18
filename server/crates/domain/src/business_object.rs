//! 業務物件的正式欄位定義
//!
//! 為什麼需要這個模組：
//!   流程 DSL 的條件式與 resolver 會引用資料路徑（`quotation.discount_rate`）。
//!   路徑打錯時求值取不到值，被當成 false，流程照跑只是少走一段簽核，
//!   沒有任何錯誤訊息——高折扣的報價單會不經財務核准就發給客戶。
//!   WF-E012 要擋這件事，而它需要一份「合法路徑」的名單。
//!
//! 為什麼不從表單定義推導：
//!   同一個 business_object 可以有多張表單，而實測顯示它們的詞彙
//!   互不相容——`quotation` 的「總計」在一張表單叫 `quotation.total`、
//!   另一張叫 `quotation.total_amount`；「內部備註」一張是單數
//!   `internal_note`、另一張是複數 `internal_notes`。
//!   取聯集會讓拼錯字合法，而拼錯字正是 WF-E012 要擋的東西。
//!
//! 目前以編譯期內嵌的 JSON 檔為來源。需要多租戶自訂時再搬進資料庫——
//! 屆時這個模組的介面不變，只換實作。

use serde::Deserialize;
use std::collections::HashSet;

/// 業務物件定義。對應 `schemas/business-objects/*.json`
#[derive(Debug, Deserialize)]
pub struct BusinessObject {
    pub business_object: String,
    pub name: String,
    pub fields: std::collections::BTreeMap<String, FieldSpec>,
}

#[derive(Debug, Deserialize)]
pub struct FieldSpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub label: Option<String>,
    pub item: Option<String>,
}

const QUOTATION: &str = include_str!("../../../../schemas/business-objects/quotation.json");
const PURCHASE_REQUEST: &str =
    include_str!("../../../../schemas/business-objects/purchase_request.json");
const LEAVE_REQUEST: &str =
    include_str!("../../../../schemas/business-objects/leave_request.json");

/// 內嵌的全部定義
///
/// 新增業務物件時在此登記。漏登記的後果是 `paths_for` 回空集合，
/// 而空集合在 WF-E012 代表「不檢查」——會靜默失去保護。
/// 所以 `known_business_objects()` 提供給呼叫端做明確性檢查。
fn all() -> Vec<BusinessObject> {
    [QUOTATION, PURCHASE_REQUEST, LEAVE_REQUEST]
        .iter()
        .map(|raw| serde_json::from_str(raw).expect("內嵌的業務物件定義應為合法 JSON"))
        .collect()
}

/// 某業務物件的合法資料路徑
///
/// 查無此業務物件時回空集合。呼叫端若需要區分
/// 「沒有定義」與「定義為空」，用 `is_known()`。
pub fn paths_for(business_object: &str) -> HashSet<String> {
    all()
        .into_iter()
        .find(|bo| bo.business_object == business_object)
        .map(|bo| bo.fields.into_keys().collect())
        .unwrap_or_default()
}

/// 是否有這個業務物件的定義
pub fn is_known(business_object: &str) -> bool {
    all().iter().any(|bo| bo.business_object == business_object)
}

/// 全部已定義的業務物件代碼
pub fn known_business_objects() -> Vec<String> {
    all().into_iter().map(|bo| bo.business_object).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_definitions_parse() {
        // 內嵌的 JSON 壞掉時要在測試就爆，而不是上線後第一次發布流程才爆
        let defs = all();
        assert_eq!(
            defs.len(),
            3,
            "應有報價單、採購申請、請假單三個定義。\
             新增定義時要同時更新 all() 與這個數字——\
             漏登記會讓該業務物件靜默失去 E012 保護"
        );
    }

    #[test]
    fn quotation_has_paths_referenced_by_fixture() {
        // fixture 流程引用的路徑必須都在定義裡，否則 E012 會讓既有流程驗不過
        let paths = paths_for("quotation");
        for p in ["quotation.discount_rate", "quotation.customer_contact_ids"] {
            assert!(paths.contains(p), "報價單定義缺少 {p}");
        }
    }

    #[test]
    fn purchase_request_has_paths_referenced_by_fixture() {
        let paths = paths_for("purchase_request");
        assert!(paths.contains("purchase_request.total"));
    }

    #[test]
    fn old_field_names_are_absent() {
        // seed.sh 的舊名稱刻意不收錄：收錄的話 E012 就擋不住拼錯字
        let paths = paths_for("quotation");
        for old in [
            "quotation.total_amount",
            "quotation.internal_notes",
            "quotation.gross_margin",
        ] {
            assert!(!paths.contains(old), "舊名稱 {old} 不該出現在正式定義");
        }
    }

    #[test]
    fn leave_request_is_known() {
        // 請假單是操作手冊示範用的第三個業務物件。
        // 沒有定義的話，照手冊建立的流程會在發布時被 E012 擋下，
        // 而使用者不會知道為什麼。
        assert!(is_known("leave_request"));

        let paths = paths_for("leave_request");
        for p in [
            "leave_request.leave_type",
            "leave_request.days",
            "leave_request.start_date",
        ] {
            assert!(paths.contains(p), "請假單定義缺少 {p}");
        }
    }

    #[test]
    fn unknown_business_object_returns_empty() {
        assert!(paths_for("nonexistent").is_empty());
        assert!(!is_known("nonexistent"));
    }
}
