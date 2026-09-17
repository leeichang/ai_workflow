//! Workflow ID 組成
//!
//! MVP 採單一 Namespace，租戶隔離靠 workflow_id 前綴
//! （見 temporal/README.md）：
//!
//!   {tenant_id}:{business_object}:{instance_id}
//!
//! 前綴不只是命名慣例，而是安全邊界。Signal 與 Cancel 都以
//! workflow_id 定位，若呼叫端能自由指定完整 id，A 租戶就能
//! 對 B 租戶的流程送 Signal。因此對外的 API 一律只收
//! business_object 與 instance_id，tenant_id 從 JWT 取得後
//! 由這裡組合，呼叫端無從干預。

use uuid::Uuid;

/// 分隔符。business_object 與 instance_id 都不允許包含它，
/// 否則能構造出跨租戶的 id。
const SEP: char = ':';

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WorkflowIdError {
    #[error("識別碼不可為空")]
    Empty,
    #[error("識別碼不可包含 '{SEP}'：{0}")]
    ContainsSeparator(String),
}

/// 組出帶租戶前綴的 workflow_id
pub fn build(
    tenant_id: Uuid,
    business_object: &str,
    instance_id: &str,
) -> Result<String, WorkflowIdError> {
    validate(business_object)?;
    validate(instance_id)?;

    Ok(format!("{tenant_id}{SEP}{business_object}{SEP}{instance_id}"))
}

fn validate(part: &str) -> Result<(), WorkflowIdError> {
    if part.is_empty() {
        return Err(WorkflowIdError::Empty);
    }
    if part.contains(SEP) {
        return Err(WorkflowIdError::ContainsSeparator(part.to_string()));
    }
    Ok(())
}

/// 檢查 workflow_id 是否屬於指定租戶
///
/// 從 Temporal 查回資料後用來確認歸屬。不信任查詢結果的
/// 來源，一律重新驗證。
pub fn belongs_to(workflow_id: &str, tenant_id: Uuid) -> bool {
    workflow_id
        .split(SEP)
        .next()
        .is_some_and(|prefix| prefix == tenant_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant() -> Uuid {
        Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
    }

    #[test]
    fn 組出三段式_id() {
        let id = build(tenant(), "quotation", "QT-2026-0001").unwrap();
        assert_eq!(
            id,
            "11111111-1111-1111-1111-111111111111:quotation:QT-2026-0001"
        );
    }

    #[test]
    fn 拒絕含分隔符的識別碼() {
        // 允許的話，呼叫端可傳入 "x:other-tenant-uuid" 之類的值
        // 構造出指向其他租戶的 id。
        let r = build(tenant(), "quotation", "QT:0001");
        assert!(matches!(r, Err(WorkflowIdError::ContainsSeparator(_))));
    }

    #[test]
    fn 拒絕空識別碼() {
        assert_eq!(build(tenant(), "", "x"), Err(WorkflowIdError::Empty));
        assert_eq!(build(tenant(), "quotation", ""), Err(WorkflowIdError::Empty));
    }

    #[test]
    fn 歸屬檢查認得自己的租戶() {
        let id = build(tenant(), "quotation", "QT-1").unwrap();
        assert!(belongs_to(&id, tenant()));
    }

    #[test]
    fn 歸屬檢查擋下其他租戶() {
        let other = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let id = build(tenant(), "quotation", "QT-1").unwrap();

        assert!(!belongs_to(&id, other));
    }

    #[test]
    fn 格式異常的_id_不視為任何租戶所有() {
        assert!(!belongs_to("", tenant()));
        assert!(!belongs_to("no-separator-here", tenant()));
    }
}
