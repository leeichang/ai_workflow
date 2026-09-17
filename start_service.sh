#!/usr/bin/env bash
#
# 啟動全部開發服務
#
# 六個元件：PostgreSQL（系統服務，僅檢查）、Temporal（Docker）、
# Rust API、Python Worker、PDF 渲染、前端。
#
# 啟動順序有相依性：Temporal 要先於 Rust API，否則 API 會以
# temporal=None 啟動，流程端點全部回 503（而且不會自己重連）。
# 同理 Rust API 要先於 Worker——Worker 啟動時就會連 internal API。
#
# 重複執行是安全的：已在跑的服務會跳過。
#
# 用法：
#   ./start_service.sh          啟動全部
#   ./start_service.sh --status 只看狀態
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$ROOT/.logs"
PID_DIR="$ROOT/.pids"
mkdir -p "$LOG_DIR" "$PID_DIR"

# ── 輸出 ────────────────────────────────────────────────

# 訊息裡變數後面接全形括號時一定要寫 ${var}。
# 寫成 "$port）" 的話，bash 會把全形字元的位元組併進變數名，
# 變成未定義的變數，在 set -u 下直接中止腳本。
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
fail() { printf '  \033[31m✗\033[0m %s\n' "$1"; }
step() { printf '\n\033[1m%s\033[0m\n' "$1"; }

port_up() { nc -z localhost "$1" 2>/dev/null; }

# 等待埠開啟。回傳 1 代表逾時。
wait_port() {
  local port=$1 name=$2 timeout=${3:-60} waited=0
  while ! port_up "$port"; do
    sleep 1
    waited=$((waited + 1))
    if [ "$waited" -ge "$timeout" ]; then
      fail "$name 等待 ${timeout}s 仍未就緒（見 ${LOG_DIR}）"
      return 1
    fi
  done
  ok "$name 就緒（:${port}）"
}

# 背景啟動並記下 PID。log 分檔，出問題時知道要看哪一個。
spawn() {
  local name=$1 log=$2
  shift 2
  nohup "$@" > "$log" 2>&1 &
  echo $! > "$PID_DIR/$name.pid"
}

# ── 狀態 ────────────────────────────────────────────────

show_status() {
  step "服務狀態"
  local rows=(
    "PostgreSQL:5432"
    "Temporal:7233"
    "Temporal UI:8080"
    "Rust API:3001"
    "PDF 渲染:3002"
    "前端:3040"
  )
  for row in "${rows[@]}"; do
    local name="${row%:*}" port="${row##*:}"
    if port_up "$port"; then
      ok "${name}（:${port}）"
    else
      fail "${name}（:${port}）未執行"
    fi
  done

  if pgrep -f "python.*worker\.py" > /dev/null; then
    ok "Python Worker"
  else
    fail "Python Worker 未執行"
  fi
}

if [ "${1:-}" = "--status" ]; then
  show_status
  exit 0
fi

# ── 環境變數 ────────────────────────────────────────────

step "載入環境變數"
if [ ! -f "$ROOT/.env" ]; then
  fail "找不到 $ROOT/.env（可從 .env.example 複製）"
  exit 1
fi
set -a
# shellcheck disable=SC1091
. "$ROOT/.env"
set +a
ok ".env 已載入"

# ── 1. PostgreSQL ───────────────────────────────────────

step "1/6 PostgreSQL"
if port_up 5432; then
  ok "已在執行（:5432）"
else
  fail "未執行。這是系統服務，請自行啟動："
  echo "      brew services start postgresql@17"
  exit 1
fi

# ── 2. Temporal ─────────────────────────────────────────

step "2/6 Temporal"
if port_up 7233; then
  ok "已在執行（:7233）"
else
  if ! docker version --format '{{.Server.Version}}' > /dev/null 2>&1; then
    warn "Docker daemon 未執行，正在啟動 Docker Desktop…"
    open -a Docker
    for _ in $(seq 1 40); do
      docker version --format '{{.Server.Version}}' > /dev/null 2>&1 && break
      sleep 3
    done
    if ! docker version --format '{{.Server.Version}}' > /dev/null 2>&1; then
      fail "Docker 等待 120s 仍未就緒"
      exit 1
    fi
    ok "Docker 就緒"
  fi

  (cd "$ROOT/poc/temporal" && docker compose up -d) > "$LOG_DIR/temporal.log" 2>&1
  wait_port 7233 "Temporal" 90 || exit 1
  wait_port 8080 "Temporal UI" 30 || warn "UI 未就緒，不影響流程執行"
fi

# ── 3. Rust API ─────────────────────────────────────────
# 必須在 Temporal 之後：API 啟動時連一次 Temporal，
# 連不上就以 temporal=None 執行，之後不會自己重連。

step "3/6 Rust API"
if port_up 3001; then
  ok "已在執行（:3001）"
else
  if [ ! -x "$ROOT/server/target/debug/api" ]; then
    warn "找不到執行檔，正在建置…"
    (cd "$ROOT/server" && cargo build --bin api) 2>&1 | tail -3
  fi
  spawn api "$LOG_DIR/api.log" "$ROOT/server/target/debug/api"
  wait_port 3001 "Rust API" 60 || { tail -20 "$LOG_DIR/api.log"; exit 1; }

  if grep -q "Temporal 已連線" "$LOG_DIR/api.log" 2>/dev/null; then
    ok "已連上 Temporal"
  else
    warn "未連上 Temporal，流程相關端點會回 503"
  fi
fi

# ── 4. Python Worker ────────────────────────────────────

step "4/6 Python Worker"
if pgrep -f "python.*worker\.py" > /dev/null; then
  ok "已在執行"
else
  if [ ! -x "$ROOT/python-ai/.venv/bin/python" ]; then
    fail "找不到 Python 虛擬環境：python-ai/.venv"
    echo "      cd python-ai && python3 -m venv .venv && .venv/bin/pip install -r requirements.txt"
    exit 1
  fi
  spawn worker "$LOG_DIR/worker.log" \
    "$ROOT/python-ai/.venv/bin/python" "$ROOT/python-ai/worker.py"

  # Worker 沒有監聽埠，改等 log 出現啟動訊息
  for _ in $(seq 1 30); do
    grep -q "Worker 啟動" "$LOG_DIR/worker.log" 2>/dev/null && break
    sleep 1
  done
  if grep -q "Worker 啟動" "$LOG_DIR/worker.log" 2>/dev/null; then
    ok "已啟動（task_queue=${TEMPORAL_TASK_QUEUE:-workflow-platform}）"
  else
    fail "啟動失敗，見 $LOG_DIR/worker.log"
    tail -10 "$LOG_DIR/worker.log"
  fi
fi

# ── 5. PDF 渲染 ─────────────────────────────────────────

step "5/6 PDF 渲染服務"
if port_up 3002; then
  ok "已在執行（:3002）"
else
  if [ ! -d "$ROOT/pdfme/node_modules" ]; then
    warn "pdfme 相依未安裝，正在安裝…"
    (cd "$ROOT/pdfme" && npm install) > "$LOG_DIR/pdfme-install.log" 2>&1
  fi
  spawn pdfme "$LOG_DIR/pdfme.log" \
    npm --prefix "$ROOT/pdfme" run dev
  wait_port 3002 "PDF 渲染" 60 || warn "未就緒，PDF 預覽不可用"
fi

# ── 6. 前端 ─────────────────────────────────────────────

step "6/6 前端"
if port_up 3040; then
  ok "已在執行（:3040）"
else
  if [ ! -d "$ROOT/web/node_modules" ]; then
    warn "web 相依未安裝，正在安裝…"
    (cd "$ROOT/web" && npm install) > "$LOG_DIR/web-install.log" 2>&1
  fi
  spawn web "$LOG_DIR/web.log" npm --prefix "$ROOT/web" run dev
  wait_port 3040 "前端" 60 || { tail -20 "$LOG_DIR/web.log"; exit 1; }
fi

# ── 完成 ────────────────────────────────────────────────

show_status

cat <<EOF

$(printf '\033[1m可用位址\033[0m')
  前端          http://localhost:3040
  Temporal UI   http://localhost:8080
  API 健康檢查  http://localhost:3001/health

$(printf '\033[1m測試帳號\033[0m')（公司代碼 demo，密碼一律 demo1234）
  designer@demo.local   陳雅婷   表單與流程設計
  cfo@demo.local        張文華   主管簽核
  finance@demo.local    李淑芬   財務簽核
  admin@demo.local      林建志   管理員

  日誌 $LOG_DIR/
  停止 ./stop_service.sh
EOF
