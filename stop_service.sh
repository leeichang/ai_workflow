#!/usr/bin/env bash
#
# 停止開發服務
#
# 預設不動 PostgreSQL 與 Docker Desktop——兩者多半還有別的用途，
# 停掉會影響其他專案。Temporal 容器則會停（它是這個專案專用的）。
#
# 用法：
#   ./stop_service.sh             停應用服務 + Temporal 容器
#   ./stop_service.sh --keep-temporal   保留 Temporal（重啟較快）
#   ./stop_service.sh --all       另外停掉 PostgreSQL
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_DIR="$ROOT/.pids"

KEEP_TEMPORAL=false
STOP_POSTGRES=false
for arg in "$@"; do
  case "$arg" in
    --keep-temporal) KEEP_TEMPORAL=true ;;
    --all)           STOP_POSTGRES=true ;;
    *) echo "未知參數：$arg"; exit 1 ;;
  esac
done

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
step() { printf '\n\033[1m%s\033[0m\n' "$1"; }

port_up() { nc -z localhost "$1" 2>/dev/null; }

# 先送 TERM 讓程序自己收尾，逾時才 KILL。
# 直接 KILL 會讓 Worker 來不及回報 Temporal，流程要等逾時才知道它走了。
stop_pid() {
  local pid=$1 name=$2
  kill "$pid" 2>/dev/null || return 1
  for _ in $(seq 1 10); do
    kill -0 "$pid" 2>/dev/null || { ok "$name 已停止"; return 0; }
    sleep 0.5
  done
  warn "$name 未回應 TERM，強制停止"
  kill -9 "$pid" 2>/dev/null
  ok "$name 已強制停止"
}

# 依 pid 檔停止。檔案裡的 PID 可能已失效（手動停過、重開機），
# 這種情況清掉檔案就好，不算錯誤。
stop_by_pidfile() {
  local name=$1 label=$2
  # 分行宣告：macOS 內建的 bash 3.2 在 set -u 下，
  # 同一個 local 敘述裡引用前面剛宣告的變數會報 unbound variable
  local file="$PID_DIR/$name.pid"
  [ -f "$file" ] || return 1
  local pid
  pid=$(cat "$file")
  if kill -0 "$pid" 2>/dev/null; then
    stop_pid "$pid" "$label"
  fi
  rm -f "$file"
  return 0
}

# pid 檔不存在時的後備：用指令特徵找。
# 腳本啟動的才有 pid 檔，手動 npm run dev 起的沒有。
stop_by_pattern() {
  local pattern=$1 label=$2
  local pids
  pids=$(pgrep -f "$pattern" 2>/dev/null) || return 1
  [ -n "$pids" ] || return 1
  for pid in $pids; do
    stop_pid "$pid" "$label"
  done
}

step "停止應用服務"

stop_by_pidfile web    "前端" ||
  stop_by_pattern "[v]ite|npm.*web run dev" "前端" ||
  warn "前端未執行"

stop_by_pidfile pdfme  "PDF 渲染" ||
  stop_by_pattern "tsx watch src/server\.ts" "PDF 渲染" ||
  warn "PDF 渲染未執行"

stop_by_pidfile worker "Python Worker" ||
  stop_by_pattern "python.*worker\.py" "Python Worker" ||
  warn "Python Worker 未執行"

stop_by_pidfile api    "Rust API" ||
  stop_by_pattern "target/debug/api" "Rust API" ||
  warn "Rust API 未執行"

# Vite 會 fork 子行程，父行程停了子行程可能還佔著埠
if port_up 3040; then
  warn "埠 3040 仍被占用，清理殘留行程"
  lsof -ti:3040 2>/dev/null | xargs kill -9 2>/dev/null
fi
if port_up 3002; then
  warn "埠 3002 仍被占用，清理殘留行程"
  lsof -ti:3002 2>/dev/null | xargs kill -9 2>/dev/null
fi

# ── Temporal ────────────────────────────────────────────

if [ "$KEEP_TEMPORAL" = true ]; then
  step "Temporal"
  ok "保留執行中（--keep-temporal）"
else
  step "停止 Temporal"
  if docker version --format '{{.Server.Version}}' > /dev/null 2>&1; then
    if [ -d "$ROOT/poc/temporal" ]; then
      # 用 stop 而非 down：保留 volume，流程歷史還在，
      # 下次啟動不必重建資料庫
      (cd "$ROOT/poc/temporal" && docker compose stop) > /dev/null 2>&1
      ok "容器已停止（資料保留）"
    fi
  else
    warn "Docker 未執行，略過"
  fi
fi

# ── PostgreSQL ──────────────────────────────────────────

if [ "$STOP_POSTGRES" = true ]; then
  step "停止 PostgreSQL"
  if brew services stop postgresql@17 > /dev/null 2>&1; then
    ok "已停止"
  else
    warn "停止失敗，請手動處理"
  fi
fi

# ── 結果 ────────────────────────────────────────────────

step "剩餘狀態"
remaining=0
for row in "PostgreSQL:5432" "Temporal:7233" "Temporal UI:8080" \
           "Rust API:3001" "PDF 渲染:3002" "前端:3040"; do
  name="${row%:*}" port="${row##*:}"
  if port_up "$port"; then
    printf '  \033[33m●\033[0m %s（:%s）仍在執行\n' "$name" "$port"
    remaining=$((remaining + 1))
  fi
done

if [ "$remaining" -eq 0 ]; then
  ok "全部已停止"
elif [ "$KEEP_TEMPORAL" = true ] || [ "$STOP_POSTGRES" = false ]; then
  echo
  echo "  PostgreSQL 與 Docker Desktop 預設不停（其他專案可能在用）"
  echo "  要一併停止 PostgreSQL：./stop_service.sh --all"
fi

echo
