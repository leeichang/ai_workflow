//! 表單定義的 schema 驗證
//!
//! 唯一真相是 `schemas/form.schema.json`。此處編譯它並提供驗證函式。
//!
//! 發布前必須驗證。草稿可以存不合法的內容（使用者編到一半），
//! 但發布代表「這份定義要被流程實際使用」，此時不能放行壞資料。

use serde_json::Value;
use std::sync::OnceLock;
use thiserror::Error;

/// 編譯後的 schema。編譯成本高，只做一次。
static FORM_SCHEMA: OnceLock<jsonschema::Validator> = OnceLock::new();

/// 內嵌 schema 檔案內容
///
/// 用 `include_str!` 而非執行時讀檔：
///   部署時不必額外帶 schemas 目錄，也不會因路徑問題在正式環境才爆炸。
///   代價是改 schema 必須重新編譯，但 schema 本來就該版本化管理。
const FORM_SCHEMA_JSON: &str = include_str!("../../../../schemas/form.schema.json");

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("表單定義不符合 schema：{0}")]
    Invalid(String),

    #[error("schema 本身有誤：{0}")]
    BadSchema(String),
}

fn validator() -> Result<&'static jsonschema::Validator, ValidationError> {
    if let Some(v) = FORM_SCHEMA.get() {
        return Ok(v);
    }

    let schema: Value = serde_json::from_str(FORM_SCHEMA_JSON)
        .map_err(|e| ValidationError::BadSchema(e.to_string()))?;
    let compiled = jsonschema::validator_for(&schema)
        .map_err(|e| ValidationError::BadSchema(e.to_string()))?;

    // 併發時可能有人先塞入，用既有的即可
    let _ = FORM_SCHEMA.set(compiled);
    Ok(FORM_SCHEMA.get().expect("剛設定過"))
}

/// 驗證表單定義
///
/// 錯誤訊息會列出所有問題而非只有第一個，讓使用者一次修完。
pub fn validate(content: &Value) -> Result<(), ValidationError> {
    let v = validator()?;

    let errors: Vec<String> = v
        .iter_errors(content)
        .map(|e| {
            let path = e.instance_path.to_string();
            let at = if path.is_empty() { "根層級".into() } else { path };
            format!("{at}：{e}")
        })
        .take(20) // 太多錯誤時截斷，避免訊息爆炸
        .collect();

    if errors.is_empty() {
        return Ok(());
    }
    Err(ValidationError::Invalid(errors.join("；")))
}

/// 額外的業務規則檢查，schema 表達不了的部分
///
/// JSON Schema 能驗結構，驗不了跨欄位的一致性。
/// 這與 workflow DSL 的兩層驗證是同樣的設計。
pub fn validate_semantics(content: &Value) -> Result<(), ValidationError> {
    let mut errors = Vec::new();

    let Some(fields) = content.get("fields").and_then(Value::as_array) else {
        return Ok(()); // 結構問題由 schema 驗證負責
    };

    // 欄位 key 不可重複
    let mut seen = std::collections::HashSet::new();
    for f in fields {
        if let Some(k) = f.get("key").and_then(Value::as_str) {
            if !seen.insert(k) {
                errors.push(format!("欄位 key 重複：{k}"));
            }
        }
    }

    // data.path 不可重複，否則兩個欄位寫同一個資料位置
    let mut paths = std::collections::HashMap::<&str, &str>::new();
    for f in fields {
        let key = f.get("key").and_then(Value::as_str).unwrap_or("?");
        if let Some(p) = f.pointer("/data/path").and_then(Value::as_str) {
            if let Some(prev) = paths.insert(p, key) {
                errors.push(format!("欄位 {prev} 與 {key} 綁定同一資料路徑：{p}"));
            }
        }
    }

    // section 參照必須存在
    let sections: std::collections::HashSet<&str> = content
        .get("sections")
        .and_then(Value::as_array)
        .map(|ss| {
            ss.iter()
                .filter_map(|s| s.get("key").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();

    if !sections.is_empty() {
        for f in fields {
            let key = f.get("key").and_then(Value::as_str).unwrap_or("?");
            if let Some(s) = f.get("section").and_then(Value::as_str) {
                if !sections.contains(s) {
                    errors.push(format!("欄位 {key} 參照不存在的 section：{s}"));
                }
            }
        }
    }

    // table 欄位必須有 columns
    for f in fields {
        let key = f.get("key").and_then(Value::as_str).unwrap_or("?");
        let is_table = f.pointer("/ui/component").and_then(Value::as_str) == Some("table");
        let has_cols = f
            .pointer("/ui/columns")
            .and_then(Value::as_array)
            .is_some_and(|c| !c.is_empty());
        if is_table && !has_cols {
            errors.push(format!("表格欄位 {key} 未定義 columns"));
        }
    }

    if errors.is_empty() {
        return Ok(());
    }
    Err(ValidationError::Invalid(errors.join("；")))
}

/// 發布前的完整檢查
pub fn validate_for_publish(content: &Value) -> Result<(), ValidationError> {
    validate(content)?;
    validate_semantics(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal() -> Value {
        json!({
            "form_key": "test_form",
            "version": 1,
            "business_object": "quotation",
            "fields": [{
                "key": "amount",
                "ui": { "component": "number", "label": "金額" },
                "data": { "path": "quotation.amount", "type": "decimal" }
            }]
        })
    }

    #[test]
    fn accepts_minimal_form() {
        assert!(validate_for_publish(&minimal()).is_ok());
    }

    #[test]
    fn rejects_missing_required_field() {
        let mut f = minimal();
        f.as_object_mut().unwrap().remove("business_object");
        assert!(validate(&f).is_err());
    }

    #[test]
    fn rejects_unknown_component() {
        let mut f = minimal();
        f["fields"][0]["ui"]["component"] = json!("richtext");
        let err = validate(&f).unwrap_err().to_string();
        assert!(err.contains("component") || err.contains("fields"), "訊息應指出問題位置：{err}");
    }

    #[test]
    fn rejects_duplicate_field_key() {
        let mut f = minimal();
        let dup = f["fields"][0].clone();
        f["fields"].as_array_mut().unwrap().push(dup);
        let err = validate_semantics(&f).unwrap_err().to_string();
        assert!(err.contains("重複"), "應報告重複：{err}");
    }

    #[test]
    fn rejects_duplicate_data_path() {
        let mut f = minimal();
        let mut other = f["fields"][0].clone();
        other["key"] = json!("amount_copy");
        f["fields"].as_array_mut().unwrap().push(other);
        let err = validate_semantics(&f).unwrap_err().to_string();
        assert!(err.contains("同一資料路徑"), "應報告路徑衝突：{err}");
    }

    #[test]
    fn rejects_unknown_section_reference() {
        let mut f = minimal();
        f["sections"] = json!([{ "key": "basic", "title": "基本" }]);
        f["fields"][0]["section"] = json!("nonexistent");
        let err = validate_semantics(&f).unwrap_err().to_string();
        assert!(err.contains("不存在的 section"), "應報告 section 問題：{err}");
    }

    #[test]
    fn rejects_table_without_columns() {
        let mut f = minimal();
        f["fields"][0]["ui"] = json!({ "component": "table", "label": "明細" });
        f["fields"][0]["data"] = json!({ "path": "quotation.lines", "type": "array" });
        let err = validate_semantics(&f).unwrap_err().to_string();
        assert!(err.contains("columns"), "應報告缺少 columns：{err}");
    }

    #[test]
    fn accepts_real_quotation_fixture() {
        let raw = include_str!("../../../../schemas/fixtures/quotation_form_v1.json");
        let content: Value = serde_json::from_str(raw).expect("fixture 應為合法 JSON");
        validate_for_publish(&content).expect("正式 fixture 應通過驗證");
    }
}
