//! 產生開發用的密碼雜湊
//!
//! 用固定 salt，因為這只是開發測試資料。
//! 正式環境的密碼由 API 的 hash_password 產生，那裡用隨機 salt。
fn main() {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let password = std::env::args().nth(1).unwrap_or_else(|| "demo1234".into());
    let salt = SaltString::from_b64("ZGV2c2FsdGRldnNhbHRkZXY").expect("固定 salt");
    let hash = argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("雜湊失敗")
        .to_string();
    println!("{hash}");
}
