//! 欄位對應建議
//!
//! 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §8。
//!
//! 客戶的 Excel 表頭是什麼都有可能：`PERNR`（SAP 的員工編號）、
//! `工號`、`員工代號`、`Emp No.`。要求管理員從 canonical 欄位清單裡
//! 一個一個對，是導入期最容易讓人放棄的一步。
//!
//! ## 為什麼是規則式而非呼叫模型
//!
//! §8 的設計是「AI 推測 → UI 顯示建議 → 人工確認 → 固化」。
//! **人工確認那一步才是關鍵**，推測只要夠準到讓人少改幾欄就有價值。
//!
//! 規則式的好處是結果穩定：同一份表頭永遠得到同一個建議。
//! 模型每次可能不同，而 mapping 一旦固化就長期沿用，漂移的代價很高。
//!
//! 介面刻意與 §8 的流程一致（產生建議、逐欄可改、確認後固化），
//! 日後換成真的模型不必改 UI 與資料結構。
//!
//! ## PII
//!
//! 這裡**只看表頭**，不看資料列。§8 要求送給 AI 的樣本必須遮蔽 PII；
//! 規則式連樣本都不需要，順帶把這個風險消掉。

use serde::Serialize;

/// canonical 欄位
///
/// 與 `app_user` / `department` 的欄位名一致。這個清單就是
/// 「同步能寫入什麼」的完整範圍——不在裡面的來源欄位一律忽略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalField {
    // 員工
    EmployeeNo,
    Name,
    Email,
    DepartmentCode,
    ManagerEmployeeNo,
    JobTitle,
    Phone,
    Extension,
    HiredAt,
    LeftAt,
    ExternalId,
    // 部門
    Code,
    ParentCode,
    ManagerEmployeeNoForDept,
}

impl CanonicalField {
    /// 寫進 `mapping_definition` 的名字
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmployeeNo => "employee_no",
            Self::Name => "name",
            Self::Email => "email",
            Self::DepartmentCode => "department_code",
            Self::ManagerEmployeeNo => "manager_employee_no",
            Self::JobTitle => "job_title",
            Self::Phone => "phone",
            Self::Extension => "extension",
            Self::HiredAt => "hired_at",
            Self::LeftAt => "left_at",
            Self::ExternalId => "external_id",
            Self::Code => "code",
            Self::ParentCode => "parent_code",
            Self::ManagerEmployeeNoForDept => "manager_employee_no",
        }
    }

    /// 給管理員看的中文名
    pub fn label(self) -> &'static str {
        match self {
            Self::EmployeeNo => "員工編號",
            Self::Name => "姓名",
            Self::Email => "電子郵件",
            Self::DepartmentCode => "部門代碼",
            Self::ManagerEmployeeNo | Self::ManagerEmployeeNoForDept => "主管員工編號",
            Self::JobTitle => "職稱",
            Self::Phone => "電話",
            Self::Extension => "分機",
            Self::HiredAt => "到職日",
            Self::LeftAt => "離職日",
            Self::ExternalId => "來源系統主鍵",
            Self::Code => "部門代碼",
            Self::ParentCode => "上層部門代碼",
        }
    }
}

/// 員工 dataset 可對應的欄位
pub const EMPLOYEE_FIELDS: &[CanonicalField] = &[
    CanonicalField::EmployeeNo,
    CanonicalField::Name,
    CanonicalField::Email,
    CanonicalField::DepartmentCode,
    CanonicalField::ManagerEmployeeNo,
    CanonicalField::JobTitle,
    CanonicalField::Phone,
    CanonicalField::Extension,
    CanonicalField::HiredAt,
    CanonicalField::LeftAt,
    CanonicalField::ExternalId,
];

/// 部門 dataset 可對應的欄位
pub const DEPARTMENT_FIELDS: &[CanonicalField] = &[
    CanonicalField::Code,
    CanonicalField::Name,
    CanonicalField::ParentCode,
    CanonicalField::ManagerEmployeeNoForDept,
    CanonicalField::ExternalId,
];

/// 同義詞表
///
/// 比對前會正規化（轉小寫、去掉空白與標點），所以這裡只列基本形。
/// 涵蓋三類來源：
///   - 台灣 SME 的中文表頭（工號、單位、分機）
///   - SAP / Oracle 的欄位代碼（PERNR、ENAME、ORGEH）
///   - 英文慣用（employee no、dept、manager）
///
/// 順序有意義：先命中先用。`manager_employee_no` 要排在
/// `employee_no` 前面，否則「主管工號」會先被 `employee_no`
/// 的「工號」吃掉。
const SYNONYMS: &[(CanonicalField, &[&str])] = &[
    (
        CanonicalField::ManagerEmployeeNo,
        &[
            "主管工號",
            "主管員工編號",
            "直屬主管",
            "主管代號",
            "上級工號",
            "managerno",
            "manageremployeeno",
            "managerid",
            "supervisor",
            "supervisorno",
            "reportsto",
            "vorna",
        ],
    ),
    (
        CanonicalField::ParentCode,
        &[
            "上層部門",
            "上級部門",
            "父部門",
            "母部門",
            "上層單位",
            "parentcode",
            "parentdept",
            "parentdepartment",
            "parentorg",
        ],
    ),
    (
        CanonicalField::EmployeeNo,
        &[
            "工號",
            "員工編號",
            "員工代號",
            "人員編號",
            "職員編號",
            "employeeno",
            "empno",
            "employeeid",
            "empid",
            "staffno",
            "staffid",
            "personnelno",
            "pernr",
            "badgeno",
        ],
    ),
    (
        CanonicalField::Name,
        &[
            "姓名",
            "名字",
            "員工姓名",
            "人員姓名",
            "name",
            "fullname",
            "employeename",
            "ename",
        ],
    ),
    (
        CanonicalField::Email,
        &[
            "電子郵件",
            "信箱",
            "郵件",
            "電子信箱",
            "email",
            "mail",
            "emailaddress",
            "mailaddress",
            "usremail",
        ],
    ),
    (
        CanonicalField::DepartmentCode,
        &[
            "部門",
            "單位",
            "部門代碼",
            "單位代碼",
            "所屬部門",
            "部門編號",
            "department",
            "dept",
            "deptcode",
            "departmentcode",
            "orgunit",
            "orgeh",
            "costcenter",
        ],
    ),
    (
        CanonicalField::Code,
        &["代碼", "編號", "部門代號", "單位代號", "code", "deptcode", "orgcode"],
    ),
    (
        CanonicalField::JobTitle,
        &[
            "職稱",
            "職位",
            "頭銜",
            "職務",
            "jobtitle",
            "title",
            "position",
            "jobposition",
            "plans",
        ],
    ),
    (
        CanonicalField::Phone,
        &[
            "電話",
            "手機",
            "聯絡電話",
            "行動電話",
            "phone",
            "mobile",
            "tel",
            "telephone",
            "cellphone",
            "contactnumber",
        ],
    ),
    (
        CanonicalField::Extension,
        &["分機", "內線", "extension", "ext", "extno"],
    ),
    (
        CanonicalField::HiredAt,
        &[
            "到職日",
            "到職日期",
            "入職日",
            "任職日",
            "hiredate",
            "hiredat",
            "joindate",
            "startdate",
            "entrydate",
            "begda",
        ],
    ),
    (
        CanonicalField::LeftAt,
        &[
            "離職日",
            "離職日期",
            "退職日",
            "終止日",
            "leavedate",
            "leftat",
            "enddate",
            "terminationdate",
            "endda",
        ],
    ),
    (
        CanonicalField::ExternalId,
        &["externalid", "sourceid", "systemid", "objectid", "guid", "uuid"],
    ),
];

/// 只在部門 dataset 生效的同義詞
///
/// 「名稱」「部門名稱」在部門表裡指的是部門名，但在**員工表**裡
/// 是另一回事——若放進共用的 SYNONYMS，一份沒有「姓名」欄的員工表
/// 會把「部門名稱」對成員工的 `name`，整份人員名單全毀，
/// 而且管理員在確認畫面上不容易看出來。
///
/// 分開列而不是靠「員工表一定有姓名欄」的假設：那個假設只要
/// 有一份表不成立就會出事，而出事的代價是整批資料寫錯。
const DEPARTMENT_ONLY_SYNONYMS: &[(CanonicalField, &[&str])] = &[(
    CanonicalField::Name,
    &[
        "名稱",
        "部門名稱",
        "單位名稱",
        "deptname",
        "departmentname",
        "orgname",
    ],
)];

/// 對一個來源欄位的建議
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    /// 來源的表頭，原樣保留
    pub source_field: String,
    /// 建議對應到哪個 canonical 欄位。None 代表猜不出來
    pub canonical_field: Option<&'static str>,
    /// 猜不出來時為 None
    pub label: Option<&'static str>,
    /// exact 完全相符、synonym 同義詞、contains 包含關係
    pub confidence: &'static str,
}

/// 正規化：轉小寫、去掉空白與常見標點
///
/// 客戶的表頭可能是 `Emp No.`、`emp_no`、`EMP-NO`，
/// 這些都該對到同一個同義詞
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !"._-()（）[]【】/\\:：#＃".contains(*c))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 產生建議
///
/// `dataset` 決定候選欄位的範圍——部門的表頭不該被建議成 `job_title`。
///
/// 一個 canonical 欄位**只會被建議一次**。Excel 常有「部門代碼」與
/// 「部門名稱」兩欄，若都建議成 `department_code`，管理員確認時
/// 兩欄會互相覆蓋，而且不容易看出問題。
pub fn suggest(headers: &[String], dataset: &str) -> Vec<Suggestion> {
    let is_department = dataset == "department";
    let candidates: &[CanonicalField] = if is_department {
        DEPARTMENT_FIELDS
    } else {
        EMPLOYEE_FIELDS
    };

    // 部門限定的同義詞排在前面：部門表裡「名稱」就是部門名，
    // 不必再去比對員工的那組
    let tables: Vec<&(CanonicalField, &[&str])> = if is_department {
        DEPARTMENT_ONLY_SYNONYMS.iter().chain(SYNONYMS).collect()
    } else {
        SYNONYMS.iter().collect()
    };

    let mut used: Vec<&'static str> = Vec::new();
    let mut result = Vec::with_capacity(headers.len());

    for header in headers {
        let normalized = normalize(header);
        let mut matched: Option<(CanonicalField, &'static str)> = None;

        for (field, synonyms) in &tables {
            if !candidates.contains(field) || used.contains(&field.as_str()) {
                continue;
            }

            for synonym in *synonyms {
                let norm_syn = normalize(synonym);
                if normalized == norm_syn {
                    matched = Some((*field, "exact"));
                    break;
                }
                // 包含關係放寬一點：「員工工號」含「工號」。
                // 但要求同義詞至少兩個字元，否則 "ext" 之類的短詞
                // 會在無關的表頭裡命中
                if matched.is_none()
                    && norm_syn.chars().count() >= 2
                    && normalized.contains(&norm_syn)
                {
                    matched = Some((*field, "contains"));
                }
            }

            // exact 直接採用；contains 要繼續找有沒有更精確的
            if matches!(matched, Some((_, "exact"))) {
                break;
            }
        }

        match matched {
            Some((field, confidence)) => {
                used.push(field.as_str());
                result.push(Suggestion {
                    source_field: header.clone(),
                    canonical_field: Some(field.as_str()),
                    label: Some(field.label()),
                    confidence,
                });
            }
            None => result.push(Suggestion {
                source_field: header.clone(),
                canonical_field: None,
                label: None,
                confidence: "none",
            }),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn mapping_of(suggestions: &[Suggestion]) -> Vec<(&str, Option<&str>)> {
        suggestions
            .iter()
            .map(|s| (s.source_field.as_str(), s.canonical_field))
            .collect()
    }

    #[test]
    fn 中文表頭() {
        let s = suggest(&headers(&["工號", "姓名", "部門", "職稱", "分機"]), "employee");

        assert_eq!(
            mapping_of(&s),
            vec![
                ("工號", Some("employee_no")),
                ("姓名", Some("name")),
                ("部門", Some("department_code")),
                ("職稱", Some("job_title")),
                ("分機", Some("extension")),
            ]
        );
    }

    #[test]
    fn sap_欄位代碼() {
        let s = suggest(&headers(&["PERNR", "ENAME", "ORGEH"]), "employee");

        assert_eq!(
            mapping_of(&s),
            vec![
                ("PERNR", Some("employee_no")),
                ("ENAME", Some("name")),
                ("ORGEH", Some("department_code")),
            ]
        );
    }

    /// 標點與大小寫不影響比對
    #[test]
    fn 正規化表頭的標點() {
        let s = suggest(&headers(&["Emp No.", "E-MAIL", "job_title"]), "employee");

        assert_eq!(
            mapping_of(&s),
            vec![
                ("Emp No.", Some("employee_no")),
                ("E-MAIL", Some("email")),
                ("job_title", Some("job_title")),
            ]
        );
    }

    /// 「主管工號」不可以被 employee_no 吃掉
    ///
    /// 兩者都含「工號」。若順序錯了，同步會把主管欄位當成員工編號，
    /// 整份組織的主管關係全錯
    #[test]
    fn 主管工號不被員工編號吃掉() {
        let s = suggest(&headers(&["工號", "主管工號"]), "employee");

        assert_eq!(
            mapping_of(&s),
            vec![
                ("工號", Some("employee_no")),
                ("主管工號", Some("manager_employee_no")),
            ]
        );
    }

    /// 順序顛倒也要對
    #[test]
    fn 主管工號排在前面也對() {
        let s = suggest(&headers(&["主管工號", "工號"]), "employee");

        assert_eq!(
            mapping_of(&s),
            vec![
                ("主管工號", Some("manager_employee_no")),
                ("工號", Some("employee_no")),
            ]
        );
    }

    /// 一個 canonical 欄位只建議一次
    ///
    /// 「部門代碼」與「部門名稱」若都對到 department_code，
    /// 管理員確認時兩欄會互相覆蓋
    #[test]
    fn 不重複建議同一個欄位() {
        let s = suggest(&headers(&["部門代碼", "部門名稱"]), "employee");

        assert_eq!(s[0].canonical_field, Some("department_code"));
        // 第二欄不可以也是 department_code
        assert_ne!(s[1].canonical_field, Some("department_code"));
    }

    /// 員工表裡的「部門名稱」不可以被建議成員工的 name
    ///
    /// 「名稱」「部門名稱」只在部門 dataset 是 name。若共用，
    /// 同步會把部門名稱寫進員工姓名——整份人員名單全毀，
    /// 而且管理員在確認畫面上不容易看出來。
    #[test]
    fn 員工表的部門名稱不對到姓名() {
        let s = suggest(&headers(&["工號", "姓名", "部門代碼", "部門名稱"]), "employee");

        assert_eq!(s[1].canonical_field, Some("name"), "姓名要對到 name");
        assert_eq!(s[2].canonical_field, Some("department_code"));
        // 第四欄猜不出來比猜錯好——管理員會看到「未對應」而去處理
        assert_eq!(
            s[3].canonical_field, None,
            "部門名稱在員工表裡沒有對應的 canonical 欄位"
        );
    }

    /// 沒有「姓名」欄時也不可以把部門名稱當成 name
    ///
    /// 上一條測試靠 `used` 機制擋住（name 已被「姓名」佔用），
    /// 但那是巧合——只要表裡沒有姓名欄，保護就消失。
    /// 這正是加上 DEPARTMENT_ONLY_SYNONYMS 的原因。
    #[test]
    fn 沒有姓名欄時部門名稱仍不對到姓名() {
        let s = suggest(&headers(&["部門代碼", "部門名稱"]), "employee");

        assert_eq!(s[0].canonical_field, Some("department_code"));
        assert_eq!(s[1].canonical_field, None, "{:?}", s[1]);
    }

    /// 但在部門 dataset 裡「名稱」就是部門名
    #[test]
    fn 部門的名稱對到_name() {
        let s = suggest(&headers(&["代碼", "名稱"]), "department");

        assert_eq!(
            mapping_of(&s),
            vec![("代碼", Some("code")), ("名稱", Some("name"))]
        );
    }

    #[test]
    fn 猜不出來的欄位回_none() {
        let s = suggest(&headers(&["備註", "自訂欄位ABC"]), "employee");

        assert_eq!(s[0].canonical_field, None);
        assert_eq!(s[0].confidence, "none");
        assert_eq!(s[1].canonical_field, None);
    }

    /// 部門 dataset 不該建議員工專屬的欄位
    #[test]
    fn 部門不建議職稱() {
        let s = suggest(&headers(&["代碼", "名稱", "職稱"]), "department");

        assert_eq!(s[0].canonical_field, Some("code"));
        assert_eq!(s[1].canonical_field, Some("name"));
        // job_title 不在 DEPARTMENT_FIELDS 裡
        assert_eq!(s[2].canonical_field, None);
    }

    #[test]
    fn 部門的上層部門() {
        let s = suggest(&headers(&["代碼", "上層部門"]), "department");

        assert_eq!(
            mapping_of(&s),
            vec![("代碼", Some("code")), ("上層部門", Some("parent_code"))]
        );
    }

    #[test]
    fn 空表頭回空清單() {
        assert!(suggest(&[], "employee").is_empty());
    }

    /// 完全相符優先於包含
    #[test]
    fn exact_優先於_contains() {
        let s = suggest(&headers(&["email"]), "employee");
        assert_eq!(s[0].confidence, "exact");
    }
}
