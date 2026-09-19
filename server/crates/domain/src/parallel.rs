//! 平行分支的結構解析
//!
//! 執行期不建 `parallel_group` 表——Temporal 自己持久化每個分支的進度，
//! 另存一份狀態就有兩份要對帳（見 `interpreter.py:_run_parallel`）。
//!
//! 代價是「這張待辦屬於哪個會簽」在資料庫裡查不到，只能從 DSL 推。
//! 本模組就做這件推導，規則**刻意與 interpreter 的走訪一致**：
//!   分支起點 = parallel 節點的所有出邊
//!   分支終點 = 從起點往下走遇到的第一個 join 節點
//!
//! 兩邊分歧的話畫面會說錯「還在等誰」，所以這裡的測試同時是
//! 對 interpreter 行為的一份說明。

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use serde_json::Value;

/// 某個節點在平行結構中的位置
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BranchInfo {
    /// 開啟這組會簽的 parallel 節點
    pub parallel_id: String,
    pub parallel_label: Option<String>,
    /// 匯合節點
    pub join_id: String,
    /// 這組會簽共有幾條分支
    pub branch_total: usize,
    /// 本節點所在分支的起點——同一分支的多個節點共用同一個起點
    pub branch_id: String,
    /// ALL / ANY / N_OF_M
    pub completion: String,
    /// N_OF_M 時的 n
    pub n: Option<u64>,
}

/// DSL 的節點與邊
struct Graph<'a> {
    nodes: HashMap<&'a str, &'a Value>,
    edges: Vec<(&'a str, &'a str)>,
}

impl<'a> Graph<'a> {
    fn parse(dsl: &'a Value) -> Option<Self> {
        let nodes: HashMap<&str, &Value> = dsl
            .get("nodes")?
            .as_array()?
            .iter()
            .filter_map(|n| Some((n.get("id")?.as_str()?, n)))
            .collect();

        let edges = dsl
            .get("edges")?
            .as_array()?
            .iter()
            .filter_map(|e| {
                let a = e.as_array()?;
                Some((a.first()?.as_str()?, a.get(1)?.as_str()?))
            })
            .collect();

        Some(Graph { nodes, edges })
    }

    fn successors(&self, id: &str) -> Vec<&'a str> {
        self.edges
            .iter()
            .filter(|(from, _)| *from == id)
            .map(|(_, to)| *to)
            .collect()
    }

    fn kind(&self, id: &str) -> Option<&str> {
        self.nodes.get(id)?.get("type")?.as_str()
    }
}

/// 從分支起點往下走，找匯合的 join
///
/// 與 `interpreter._find_join` 同一套走訪：分支可以有多個節點、
/// 條件判斷、巢狀並行，因此要 BFS 而非只看直接後繼。
fn find_join(g: &Graph, start: &str) -> Option<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::from([start]);

    while let Some(current) = queue.pop_front() {
        if !seen.insert(current) {
            continue;
        }
        if g.kind(current) == Some("join") {
            return Some(current.to_string());
        }
        for next in g.successors(current) {
            queue.push_back(next);
        }
    }
    None
}

/// 某條分支涵蓋的所有節點（不含 join 本身）
fn branch_members(g: &Graph, start: &str) -> HashSet<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::from([start]);
    let mut out = HashSet::new();

    while let Some(current) = queue.pop_front() {
        if !seen.insert(current) {
            continue;
        }
        // join 是匯合點，屬於整組而非某一條分支
        if g.kind(current) == Some("join") {
            continue;
        }
        out.insert(current.to_string());
        for next in g.successors(current) {
            queue.push_back(next);
        }
    }
    out
}

/// 查某個節點屬於哪組會簽的哪條分支
///
/// 不屬於任何平行結構時回 `None`——多數節點都是這樣，
/// 呼叫端據此決定要不要顯示會簽進度。
pub fn branch_of(dsl: &Value, node_id: &str) -> Option<BranchInfo> {
    let g = Graph::parse(dsl)?;

    for (&pid, pnode) in &g.nodes {
        if pnode.get("type").and_then(Value::as_str) != Some("parallel") {
            continue;
        }
        let starts = g.successors(pid);
        if starts.is_empty() {
            continue;
        }
        let Some(join_id) = starts.iter().find_map(|s| find_join(&g, s)) else {
            continue;
        };

        for start in &starts {
            if !branch_members(&g, start).contains(node_id) {
                continue;
            }
            let join_node = g.nodes.get(join_id.as_str());
            let policy = join_node.and_then(|n| n.get("join"));
            return Some(BranchInfo {
                parallel_id: pid.to_string(),
                parallel_label: pnode
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                join_id,
                branch_total: starts.len(),
                branch_id: start.to_string(),
                completion: policy
                    .and_then(|p| p.get("completion"))
                    .and_then(Value::as_str)
                    .unwrap_or("ALL")
                    .to_string(),
                n: policy.and_then(|p| p.get("n")).and_then(Value::as_u64),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 財務與法務會簽，兩條分支匯合到同一個 join
    fn cosign_dsl() -> Value {
        json!({
            "nodes": [
                {"id": "start", "type": "trigger"},
                {"id": "cosign", "type": "parallel", "label": "財務與法務會簽"},
                {"id": "finance_approval", "type": "human_approval"},
                {"id": "legal_approval", "type": "human_approval"},
                {"id": "cosign_join", "type": "join",
                 "join": {"completion": "ALL", "result": "ALL_SUCCESS"}},
                {"id": "end", "type": "end"}
            ],
            "edges": [
                ["start", "cosign"],
                ["cosign", "finance_approval"],
                ["cosign", "legal_approval"],
                ["finance_approval", "cosign_join"],
                ["legal_approval", "cosign_join"],
                ["cosign_join", "end"]
            ]
        })
    }

    #[test]
    fn both_branches_report_the_same_join_and_total() {
        // 兩張待辦要被認成同一組會簽，畫面才說得出「2 條中完成 1 條」。
        // 認成兩組獨立的事，使用者會以為流程卡住了。
        let dsl = cosign_dsl();
        let fin = branch_of(&dsl, "finance_approval").expect("財務應屬於會簽");
        let legal = branch_of(&dsl, "legal_approval").expect("法務應屬於會簽");

        assert_eq!(fin.join_id, legal.join_id);
        assert_eq!(fin.parallel_id, legal.parallel_id);
        assert_eq!(fin.branch_total, 2);
        assert_eq!(fin.completion, "ALL");
        // 但分支本身不同——否則無從分辨誰還沒簽
        assert_ne!(fin.branch_id, legal.branch_id);
    }

    #[test]
    fn node_outside_any_parallel_has_no_branch() {
        // 大多數節點都不在平行結構裡。硬給一組會簽資訊
        // 會讓畫面顯示不存在的「等待其他分支」。
        assert!(branch_of(&cosign_dsl(), "start").is_none());
    }

    #[test]
    fn join_itself_belongs_to_no_branch() {
        // join 是匯合點，屬於整組而非某一條分支
        assert!(branch_of(&cosign_dsl(), "cosign_join").is_none());
    }

    #[test]
    fn multi_node_branch_is_traced_to_its_join() {
        // 分支可以有多個節點。只看直接後繼會漏掉第二個節點，
        // 那張待辦就顯示不出會簽進度。
        let dsl = json!({
            "nodes": [
                {"id": "p", "type": "parallel"},
                {"id": "a1", "type": "human_approval"},
                {"id": "a2", "type": "human_approval"},
                {"id": "b1", "type": "human_approval"},
                {"id": "j", "type": "join", "join": {"completion": "ALL"}}
            ],
            "edges": [
                ["p", "a1"], ["a1", "a2"], ["a2", "j"],
                ["p", "b1"], ["b1", "j"]
            ]
        });

        let deep = branch_of(&dsl, "a2").expect("分支第二個節點也要認得出來");
        assert_eq!(deep.join_id, "j");
        // 分支起點是 a1，不是 a2——同一條分支共用一個起點
        assert_eq!(deep.branch_id, "a1");
        assert_eq!(deep.branch_total, 2);
    }

    #[test]
    fn n_of_m_policy_is_carried_through() {
        // 「5 人中 3 人同意」要顯示成 3，顯示成 5 會讓使用者
        // 多簽兩關才發現流程早就過了
        let dsl = json!({
            "nodes": [
                {"id": "p", "type": "parallel"},
                {"id": "a", "type": "human_approval"},
                {"id": "b", "type": "human_approval"},
                {"id": "j", "type": "join", "join": {"completion": "N_OF_M", "n": 2}}
            ],
            "edges": [["p", "a"], ["a", "j"], ["p", "b"], ["b", "j"]]
        });

        let info = branch_of(&dsl, "a").unwrap();
        assert_eq!(info.completion, "N_OF_M");
        assert_eq!(info.n, Some(2));
    }

    #[test]
    fn parallel_without_join_is_ignored_not_panicking() {
        // DSL 驗證器會擋這種流程，但舊資料可能還在。
        // 畫面少一塊資訊可以接受，整頁壞掉不行。
        let dsl = json!({
            "nodes": [
                {"id": "p", "type": "parallel"},
                {"id": "a", "type": "human_approval"}
            ],
            "edges": [["p", "a"]]
        });
        assert!(branch_of(&dsl, "a").is_none());
    }
}
