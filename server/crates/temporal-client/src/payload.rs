//! Payload 編碼
//!
//! Rust 端送出的 Payload 必須讓 Python 的 `temporalio` 預設
//! DataConverter 解得開，反之亦然。這是決議 D-02 的風險點：
//! 兩端各自實作編碼，不相容時的症狀是 Workflow 收到 None
//! 或型別錯誤，而不是明確的解碼失敗。
//!
//! Python 端的規則（見 reference/temporal-sdk-python/tests/test_converter.py）：
//!   None      → encoding = binary/null，data 為空
//!   其餘 JSON → encoding = json/plain，data 為 JSON 位元組
//!
//! null 是特例：不是 `json/plain` 的字串 "null"，而是獨立的
//! `binary/null` 編碼且 data 為空。送錯的話 Python 會解成
//! 字串而非 None。

use crate::proto::temporal::api::common::v1::{Payload, Payloads};
use serde_json::Value;
use std::collections::HashMap;

const ENCODING_KEY: &str = "encoding";
const JSON_PLAIN: &[u8] = b"json/plain";
const BINARY_NULL: &[u8] = b"binary/null";

/// 將 JSON 值編碼為 Temporal Payload
pub fn encode(value: &Value) -> Payload {
    let mut metadata = HashMap::new();

    // 用 ..Default::default() 而非列舉所有欄位：Temporal 的 proto
    // 會新增欄位（例如 external_payloads），列舉法每次升級都會編譯失敗。
    if value.is_null() {
        metadata.insert(ENCODING_KEY.to_string(), BINARY_NULL.to_vec());
        return Payload {
            metadata,
            data: Vec::new(),
            ..Default::default()
        };
    }

    metadata.insert(ENCODING_KEY.to_string(), JSON_PLAIN.to_vec());
    Payload {
        metadata,
        // serde_json 不保證鍵順序與 Python 一致，但 JSON 物件無序，
        // 兩端解出來的字典相同，不影響正確性。
        data: serde_json::to_vec(value).unwrap_or_default(),
        ..Default::default()
    }
}

/// 將多個 JSON 值編碼為 Payloads
///
/// Temporal 的 Workflow 參數是位置引數列表。單一物件參數
/// 仍要包成只有一個元素的 Payloads。
pub fn encode_all(values: &[Value]) -> Payloads {
    Payloads {
        payloads: values.iter().map(encode).collect(),
    }
}

/// 解碼單一 Payload
///
/// 無法辨識的編碼回傳 None 而非猜測。Temporal 支援自訂編碼
/// （壓縮、加密），猜錯會產生難以追查的資料損壞。
pub fn decode(payload: &Payload) -> Option<Value> {
    let encoding = payload.metadata.get(ENCODING_KEY)?;

    match encoding.as_slice() {
        BINARY_NULL => Some(Value::Null),
        JSON_PLAIN => serde_json::from_slice(&payload.data).ok(),
        _ => None,
    }
}

/// 解碼 Payloads，取第一個
///
/// Workflow 的回傳值與 Query 結果都是單一值，
/// 但協定上仍是列表。空列表視為 null。
pub fn decode_first(payloads: Option<&Payloads>) -> Value {
    payloads
        .and_then(|p| p.payloads.first())
        .and_then(decode)
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn encoding_of(p: &Payload) -> &[u8] {
        p.metadata.get(ENCODING_KEY).unwrap()
    }

    #[test]
    fn 物件編碼為_json_plain() {
        let p = encode(&json!({ "amount": 1000 }));

        assert_eq!(encoding_of(&p), JSON_PLAIN);
        assert_eq!(
            serde_json::from_slice::<Value>(&p.data).unwrap(),
            json!({ "amount": 1000 })
        );
    }

    #[test]
    fn null_編碼為_binary_null_且_data_為空() {
        // Python 端把 None 編成 binary/null 而非 json/plain 的 "null"。
        // 送成後者的話，Python 會解出字串 "null" 而不是 None，
        // 而且不會有任何錯誤。
        let p = encode(&Value::Null);

        assert_eq!(encoding_of(&p), BINARY_NULL);
        assert!(p.data.is_empty(), "binary/null 的 data 必須為空");
    }

    #[test]
    fn 字串編碼為帶引號的_json() {
        let p = encode(&json!("somestr"));
        assert_eq!(p.data, b"\"somestr\"");
    }

    #[test]
    fn 數字與布林不加引號() {
        assert_eq!(encode(&json!(1234)).data, b"1234");
        assert_eq!(encode(&json!(12.34)).data, b"12.34");
        assert_eq!(encode(&json!(true)).data, b"true");
        assert_eq!(encode(&json!(false)).data, b"false");
    }

    #[test]
    fn 編碼解碼往返一致() {
        let cases = vec![
            json!(null),
            json!(true),
            json!(1234),
            json!("字串"),
            json!([1, 2, 3]),
            json!({ "nested": { "a": 1 }, "list": [1, "2"] }),
        ];

        for c in cases {
            assert_eq!(decode(&encode(&c)), Some(c.clone()), "往返後應相同：{c}");
        }
    }

    #[test]
    fn 未知編碼回傳_none_而非猜測() {
        // Temporal 支援自訂編碼（壓縮、加密）。猜測會造成資料損壞。
        let mut metadata = HashMap::new();
        metadata.insert(ENCODING_KEY.to_string(), b"binary/encrypted".to_vec());
        let p = Payload {
            metadata,
            data: b"whatever".to_vec(),
            ..Default::default()
        };

        assert_eq!(decode(&p), None);
    }

    #[test]
    fn 缺少編碼標記回傳_none() {
        let p = Payload {
            metadata: HashMap::new(),
            data: b"{}".to_vec(),
            ..Default::default()
        };

        assert_eq!(decode(&p), None);
    }

    #[test]
    fn 多個參數各自成為一個_payload() {
        let ps = encode_all(&[json!({ "a": 1 }), json!("second")]);
        assert_eq!(ps.payloads.len(), 2);
    }

    #[test]
    fn 空_payloads_視為_null() {
        assert_eq!(decode_first(None), Value::Null);
        assert_eq!(
            decode_first(Some(&Payloads { payloads: vec![] })),
            Value::Null
        );
    }
}
