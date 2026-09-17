//! 由 Temporal 的 .proto 產生 gRPC client
//!
//! proto 檔複製自 reference/temporal-api（Temporal 官方 API 定義，MIT）。
//! 刻意複製進本 crate 而非指向 reference/：後者不納入版控，
//! 少了它就無法建置，clone 下來的專案會直接壞掉。
//!
//! 只產生 client，不產生 server。我們是 Temporal 的使用者，
//! 不需要實作 WorkflowService。

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "proto";

    // 只編譯 WorkflowService。其餘服務（operator、nexus 等）本專案用不到，
    // 全部編譯會讓建置時間與產物大幅膨脹。
    //
    // 但 protoc 會連帶產生它所依賴的全部 package，數量比直覺多得多
    // （目前 29 個）。因此模組宣告改為掃描產物自動生成，
    // 手寫清單漏一個就是編譯錯誤。
    let services = &[format!(
        "{proto_root}/temporal/api/workflowservice/v1/service.proto"
    )];

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(services, &[proto_root])?;

    generate_module_tree()?;

    println!("cargo:rerun-if-changed={proto_root}");
    Ok(())
}

/// 依產物檔名生成巢狀 module 宣告
///
/// tonic 產生的檔名是 `<package>.rs`，例如 `temporal.api.common.v1.rs`。
/// `include_proto!` 必須放在對應的 module 層級，因此要把扁平的
/// package 名還原成巢狀結構。
fn generate_module_tree() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::var("OUT_DIR")?;

    let mut packages = BTreeSet::new();
    for entry in std::fs::read_dir(&out_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // 跳過本函式自己產生的檔案，否則會遞迴納入
        if name == "_modules" {
            continue;
        }
        packages.insert(name.to_string());
    }

    let mut code = String::new();
    write_tree(&mut code, &packages, &[]);

    std::fs::write(Path::new(&out_dir).join("_modules.rs"), code)?;
    Ok(())
}

/// 遞迴輸出某個前綴之下的 module
fn write_tree(code: &mut String, packages: &BTreeSet<String>, prefix: &[&str]) {
    // 找出此層級的所有 segment
    let mut segments = BTreeSet::new();
    for pkg in packages {
        let parts: Vec<&str> = pkg.split('.').collect();
        if parts.len() <= prefix.len() || !parts.starts_with(prefix) {
            continue;
        }
        segments.insert(parts[prefix.len()].to_string());
    }

    for seg in segments {
        let mut path: Vec<&str> = prefix.to_vec();
        path.push(&seg);
        let full = path.join(".");

        let _ = writeln!(code, "pub mod {} {{", sanitize(&seg));

        // 這一層本身就是一個 package 時，放入對應的產物
        if packages.contains(&full) {
            let _ = writeln!(code, "    tonic::include_proto!(\"{full}\");");
        }

        write_tree(code, packages, &path);
        let _ = writeln!(code, "}}");
    }
}

/// proto package 的 segment 可能與 Rust 關鍵字衝突
fn sanitize(seg: &str) -> String {
    match seg {
        "type" | "match" | "move" | "ref" | "self" | "super" | "crate" | "mod" | "box" => {
            format!("r#{seg}")
        }
        other => other.to_string(),
    }
}
