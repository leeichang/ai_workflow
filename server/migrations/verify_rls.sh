#!/usr/bin/env bash
#
# 驗證 Row Level Security 租戶隔離
#
# 任何修改 RLS、新增帶 tenant_id 的表、或調整連線角色之後，
# 都必須重跑此腳本。
#
# 用法：./server/migrations/verify_rls.sh
set -uo pipefail

HOST="${DB_HOST:-localhost}"
PORT="${DB_PORT:-5433}"
DB="${DB_NAME:-workflow}"

# 管理連線。容器環境用 app，本機 Homebrew 安裝通常是作業系統帳號。
# 可用 ADMIN_DATABASE_URL 完整覆寫。
ADMIN_URL="${ADMIN_DATABASE_URL:-postgres://app:${ADMIN_PASSWORD:-app_dev_only}@${HOST}:${PORT}/${DB}}"
APP_URL="${APP_DATABASE_URL:-postgres://workflow_app:${APP_PASSWORD:-app_dev_only}@${HOST}:${PORT}/${DB}}"

A='aaaaaaaa-0000-0000-0000-000000000001'
B='bbbbbbbb-0000-0000-0000-000000000002'

pass=0
fail=0

check() {
    local name="$1" expected="$2" actual="$3"
    if [[ "$actual" == "$expected" ]]; then
        printf '  \033[32mPASS\033[0m  %s\n' "$name"
        ((pass++))
    else
        printf '  \033[31mFAIL\033[0m  %s\n        期望 [%s] 實際 [%s]\n' "$name" "$expected" "$actual"
        ((fail++))
    fi
}

cleanup() {
    psql "$ADMIN_URL" -q -c "delete from app_user where tenant_id in ('$A','$B');
                             delete from tenant where id in ('$A','$B');" >/dev/null 2>&1
}
trap cleanup EXIT

echo "驗證 RLS 租戶隔離"
echo

# ── 前置：確認連線角色不是 superuser ──────────────────────
is_super=$(psql "$APP_URL" -tA -c \
    "select usesuper from pg_user where usename = current_user;" 2>&1 | tr -d ' ')
check "應用程式角色不是 superuser" "f" "$is_super"

if [[ "$is_super" != "f" ]]; then
    echo
    echo "  superuser 會完全繞過 RLS，後續測試無意義。"
    echo "  請確認應用程式以 workflow_app 連線，而非 app。"
    exit 1
fi

# ── 建立測試資料 ─────────────────────────────────────────
cleanup
psql "$ADMIN_URL" -q -c "
    insert into tenant (id, code, name) values
      ('$A', 'rls-test-a', 'RLS 測試 A'),
      ('$B', 'rls-test-b', 'RLS 測試 B');" >/dev/null 2>&1

for pair in "$A:a@rls-test.local:甲使用者" "$B:b@rls-test.local:乙使用者"; do
    IFS=: read -r tid email name <<< "$pair"
    psql "$APP_URL" -q -c "
        begin;
        set local app.tenant_id = '$tid';
        insert into app_user (tenant_id, email, name, password_hash)
        values ('$tid', '$email', '$name', 'test');
        commit;" >/dev/null 2>&1
done

q() {  # $1 = tenant_id, $2 = SQL
    psql "$APP_URL" -tA -c "begin; set local app.tenant_id='$1'; $2; commit;" 2>&1 \
        | grep -vE '^(BEGIN|COMMIT|SET)$' | grep -v '^$' | head -1 | tr -d ' '
}

# ── 隔離測試 ─────────────────────────────────────────────
echo
check "租戶 A 只看到自己的使用者" "1" \
      "$(q "$A" "select count(*) from app_user where email like '%@rls-test.local'")"

check "租戶 B 只看到自己的使用者" "1" \
      "$(q "$B" "select count(*) from app_user where email like '%@rls-test.local'")"

check "租戶 A 看不到 B 的 email" "" \
      "$(q "$A" "select email from app_user where email = 'b@rls-test.local'")"

# ── fail-closed ─────────────────────────────────────────
check "未設定 tenant_id 時查詢回空" "0" \
      "$(psql "$APP_URL" -tA -c "select count(*) from app_user;" 2>&1 | tr -d ' ')"

# ── 寫入檢查 ─────────────────────────────────────────────
cross=$(psql "$APP_URL" -tA -c "
    begin;
    set local app.tenant_id = '$A';
    insert into app_user (tenant_id, email, name, password_hash)
    values ('$B', 'cross@rls-test.local', '跨租戶', 'x');
    commit;" 2>&1 | grep -ciE "policy|錯誤|error" || true)
check "以租戶 A 身份寫入 B 的資料被拒" "1" "$cross"

# ── 稽核表不可修改 ───────────────────────────────────────
psql "$APP_URL" -q -c "
    begin; set local app.tenant_id='$A';
    insert into audit_event (tenant_id, actor_kind, action, target_type)
    values ('$A', 'system', 'rls.test', 'test');
    commit;" >/dev/null 2>&1

no_upd=$(psql "$APP_URL" -tA -c "
    begin; set local app.tenant_id='$A';
    update audit_event set action='tampered' where action='rls.test';
    commit;" 2>&1 | grep -ciE "不可|denied|錯誤|error" || true)
check "稽核事件不可修改" "1" "$no_upd"

psql "$ADMIN_URL" -q -c "delete from audit_event where action in ('rls.test','tampered');" >/dev/null 2>&1

# ── 涵蓋率：所有帶 tenant_id 的表都要有 policy ───────────
echo
missing=$(psql "$ADMIN_URL" -tA -c "
    select string_agg(c.relname, ', ')
    from pg_class c
    join pg_namespace n on n.oid = c.relnamespace
    join pg_attribute a on a.attrelid = c.oid and a.attname = 'tenant_id'
    where n.nspname = 'public' and c.relkind = 'r'
      and not exists (select 1 from pg_policies p
                      where p.tablename = c.relname and p.schemaname = 'public');" 2>&1 | tr -d ' ')
check "所有帶 tenant_id 的表都有 policy" "" "$missing"

not_forced=$(psql "$ADMIN_URL" -tA -c "
    select string_agg(c.relname, ', ')
    from pg_class c
    join pg_namespace n on n.oid = c.relnamespace
    join pg_attribute a on a.attrelid = c.oid and a.attname = 'tenant_id'
    where n.nspname = 'public' and c.relkind = 'r'
      and not (c.relrowsecurity and c.relforcerowsecurity);" 2>&1 | tr -d ' ')
check "所有帶 tenant_id 的表都 FORCE RLS" "" "$not_forced"

echo
if (( fail > 0 )); then
    printf '\033[31m%d 項失敗\033[0m，%d 項通過\n' "$fail" "$pass"
    exit 1
fi
printf '\033[32m全部 %d 項通過\033[0m\n' "$pass"
