//! 匯入檔的解析與映射
//!
//! 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §5。
//!
//! 管線的 Parse 與 Map 兩段。刻意不碰資料庫——這一層只負責把
//! 「客戶的檔案」變成「canonical 的列」，驗證與寫入在 persistence。
//!
//! ## 為什麼只做 CSV
//!
//! 需求寫的是「Excel / CSV」，本輪只實作 CSV。xlsx 需要額外的
//! 解析依賴，而每個 Excel 都另存得出 CSV。管線做對之後，
//! 補 xlsx 只是換一個產生 `(headers, rows)` 的函式。
//!
//! 這個縮減要讓使用者看得到：UI 的上傳說明要寫「請另存為 CSV」，
//! 而不是讓人上傳 xlsx 之後才拿到看不懂的錯誤。

use serde::Serialize;
use std::collections::HashMap;

/// 解析出來的表格
#[derive(Debug, Clone)]
pub struct Sheet {
    pub headers: Vec<String>,
    /// 每一列。長度與 headers 對齊——短的列補空字串，
    /// 長的列截掉，否則後面用 index 取值會 panic
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("檔案不是有效的 CSV：{0}")]
    Malformed(String),

    #[error("檔案沒有標題列")]
    NoHeader,

    #[error("檔案沒有任何資料列")]
    NoRows,

    #[error("標題列有重複的欄位「{0}」")]
    DuplicateHeader(String),
}

/// 一次匯入的列數上限
///
/// SME 的組織規模是千人等級。設上限是為了擋下誤傳的大檔——
/// 沒有上限時，一個 500MB 的檔案會把整個 API 的記憶體吃掉。
pub const MAX_ROWS: usize = 50_000;

/// 解析 CSV
///
/// BOM 會被去掉：Excel 另存 UTF-8 CSV 時會加 BOM，
/// 不處理的話第一個欄位名會變成「\u{feff}工號」而對應不到任何同義詞。
pub fn parse_csv(content: &[u8]) -> Result<Sheet, ImportError> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(strip_bom(content));

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ImportError::Malformed(e.to_string()))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();

    if headers.is_empty() || headers.iter().all(|h| h.is_empty()) {
        return Err(ImportError::NoHeader);
    }

    // 重複的表頭會讓 mapping 有歧義——兩欄都叫「工號」時，
    // 該用哪一欄的值？與其猜，不如要求客戶先修檔案
    let mut seen = Vec::new();
    for header in &headers {
        if header.is_empty() {
            continue;
        }
        if seen.contains(header) {
            return Err(ImportError::DuplicateHeader(header.clone()));
        }
        seen.push(header.clone());
    }

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| ImportError::Malformed(e.to_string()))?;

        let mut row: Vec<String> = record.iter().map(|v| v.trim().to_string()).collect();
        // 對齊長度。flexible(true) 允許長短不一的列，
        // 但後面用 index 取值，長度不對就會 panic
        row.resize(headers.len(), String::new());
        rows.push(row);

        if rows.len() > MAX_ROWS {
            return Err(ImportError::Malformed(format!(
                "資料列超過 {MAX_ROWS} 列的上限"
            )));
        }
    }

    if rows.is_empty() {
        return Err(ImportError::NoRows);
    }

    Ok(Sheet { headers, rows })
}

/// Excel 另存 UTF-8 CSV 時會加的位元組順序記號
fn strip_bom(content: &[u8]) -> &[u8] {
    content.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(content)
}

/// 映射後的一列
///
/// key 是 canonical 欄位名，value 是原始字串。
/// 型別轉換（日期）留到驗證那一步——這裡轉的話，
/// 錯誤訊息會失去「第幾列、哪個欄位」的脈絡。
#[derive(Debug, Clone, Serialize)]
pub struct MappedRow {
    /// 來源的第幾列（1-based，不含標題列）。
    /// 管理員要能回到 Excel 找到那一行
    pub row_number: usize,
    pub values: HashMap<String, String>,
}

/// 依 mapping 把原始列轉成 canonical 列
///
/// `mapping` 是「來源欄位名 → canonical 欄位名」。
/// 不在 mapping 裡的來源欄位一律忽略——客戶的表常有平台用不到的欄
/// （部門主管的分機、成本中心），那不是錯誤。
///
/// 空字串會被略過而非存成空值：Excel 的空白儲存格與「刻意清空」
/// 分不出來，而把既有資料清掉的風險遠大於漏更新一欄。
pub fn map_rows(sheet: &Sheet, mapping: &HashMap<String, String>) -> Vec<MappedRow> {
    // 先算出每個 canonical 欄位在第幾個 column，避免逐列查表
    let columns: Vec<(usize, &String)> = sheet
        .headers
        .iter()
        .enumerate()
        .filter_map(|(index, header)| mapping.get(header).map(|target| (index, target)))
        .collect();

    sheet
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut values = HashMap::new();
            for (column, target) in &columns {
                let value = &row[*column];
                if !value.is_empty() {
                    values.insert((*target).clone(), value.clone());
                }
            }
            MappedRow {
                row_number: index + 1,
                values,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn 解析基本的_csv() {
        let sheet = parse_csv(b"\xe5\xb7\xa5\xe8\x99\x9f,name\nE001,Alice\nE002,Bob\n").unwrap();

        assert_eq!(sheet.headers, vec!["工號", "name"]);
        assert_eq!(sheet.rows.len(), 2);
        assert_eq!(sheet.rows[0], vec!["E001", "Alice"]);
    }

    /// Excel 另存 UTF-8 CSV 會加 BOM
    ///
    /// 不去掉的話第一個欄位名是「\u{feff}工號」，對應不到任何同義詞，
    /// 而管理員只會看到「員工編號未對應」，完全猜不到原因
    #[test]
    fn 去掉_bom() {
        let mut content = vec![0xEF, 0xBB, 0xBF];
        content.extend_from_slice("工號,姓名\nE001,陳小明\n".as_bytes());

        let sheet = parse_csv(&content).unwrap();
        assert_eq!(sheet.headers[0], "工號");
    }

    #[test]
    fn 去掉欄位前後的空白() {
        let sheet = parse_csv(b" code , name \n A01 , Sales \n").unwrap();

        assert_eq!(sheet.headers, vec!["code", "name"]);
        assert_eq!(sheet.rows[0], vec!["A01", "Sales"]);
    }

    /// 引號內的逗號不是分隔符
    #[test]
    fn 處理引號內的逗號() {
        let sheet = parse_csv(b"code,name\nA01,\"Sales, North\"\n").unwrap();
        assert_eq!(sheet.rows[0][1], "Sales, North");
    }

    /// 短的列要補齊，否則後面用 index 取值會 panic
    #[test]
    fn 補齊長度不足的列() {
        let sheet = parse_csv(b"a,b,c\n1,2\n").unwrap();

        assert_eq!(sheet.rows[0], vec!["1", "2", ""]);
    }

    /// 重複的表頭要擋下而不是猜
    #[test]
    fn 拒絕重複的表頭() {
        let err = parse_csv(b"code,code\n1,2\n").unwrap_err();
        assert!(matches!(err, ImportError::DuplicateHeader(_)), "{err}");
    }

    #[test]
    fn 拒絕沒有資料列的檔案() {
        let err = parse_csv(b"code,name\n").unwrap_err();
        assert!(matches!(err, ImportError::NoRows), "{err}");
    }

    #[test]
    fn 拒絕空檔案() {
        let err = parse_csv(b"").unwrap_err();
        assert!(matches!(err, ImportError::NoHeader), "{err}");
    }

    #[test]
    fn 映射只取_mapping_裡的欄位() {
        let sheet = parse_csv(b"emp,name,memo\nE001,Alice,ignore me\n").unwrap();
        let m = mapping(&[("emp", "employee_no"), ("name", "name")]);

        let rows = map_rows(&sheet, &m);

        assert_eq!(rows[0].values.get("employee_no").unwrap(), "E001");
        assert_eq!(rows[0].values.get("name").unwrap(), "Alice");
        // 客戶的表常有平台用不到的欄，那不是錯誤
        assert!(!rows[0].values.contains_key("memo"));
    }

    /// 空儲存格不進 values
    ///
    /// Excel 的空白與「刻意清空」分不出來。把既有資料清掉的風險
    /// 遠大於漏更新一欄
    #[test]
    fn 空值不覆蓋既有資料() {
        let sheet = parse_csv(b"emp,title\nE001,\n").unwrap();
        let m = mapping(&[("emp", "employee_no"), ("title", "job_title")]);

        let rows = map_rows(&sheet, &m);

        assert_eq!(rows[0].values.get("employee_no").unwrap(), "E001");
        assert!(!rows[0].values.contains_key("job_title"));
    }

    /// row_number 從 1 開始，且不含標題列
    ///
    /// 管理員要能回到 Excel 找到那一行
    #[test]
    fn row_number_對應到檔案的行號() {
        let sheet = parse_csv(b"emp\nE001\nE002\n").unwrap();
        let m = mapping(&[("emp", "employee_no")]);

        let rows = map_rows(&sheet, &m);

        assert_eq!(rows[0].row_number, 1);
        assert_eq!(rows[1].row_number, 2);
    }
}
