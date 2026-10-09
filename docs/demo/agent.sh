#!/usr/bin/env bash
# Имитация AI-агента для демо-записи (docs/demo/record.py). Это НЕ настоящий агент:
# печатает «работу», правит файлы в git-репозитории (чтобы Radar показал ветку, +/−, diff),
# шлёт в Radar те же события хуков, что и Claude Code (UserPromptSubmit, PermissionRequest, Stop),
# и один раз рисует меню запроса разрешения. Сценарий выбирается по имени папки: api / web / docs.
name=$(basename "$PWD")
F=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)
ORANGE=$'\033[38;5;208m'; DIM=$'\033[2m'; GREEN=$'\033[32m'; BOLD=$'\033[1m'; BLUE=$'\033[38;5;111m'; R=$'\033[0m'

hook() { # hook <Событие> <json>
  [ -n "$RADAR_DEMO_BIN" ] && printf '%s' "${2:-{\}}" | "$RADAR_DEMO_BIN" hook "$1" >/dev/null 2>&1
}
spin() { # spin <тактов по 0.2 c> <текст>
  local i
  for ((i = 0; i < $1; i++)); do
    printf '\r\033[K%s%s%s %s %s(esc to interrupt)%s' "$ORANGE" "${F[i % 10]}" "$R" "$2" "$DIM" "$R"
    sleep 0.2
  done
  printf '\r\033[K'
}
step() { printf '%s●%s %s\n' "$DIM" "$R" "$1"; hook PreToolUse '{"tool_name":"demo"}'; }

printf '%s●%s demo agent · %s\n%s  (симуляция вывода, не настоящий агент)%s\n\n> ' "$ORANGE" "$R" "$name" "$DIM" "$R"
IFS= read -r task
hook UserPromptSubmit "{\"prompt\":\"${task//\"/\\\"}\"}"
printf '\n'

case "$name" in
api)
  step "Read(src/server.ts)";            spin 6 "Анализирую маршруты…"
  sed -i '/^\/\/ TODO: health/d' src/server.ts
  cat >> src/server.ts <<'EOF'

// health-check для балансировщика
app.get('/health', (_req, res) => {
  res.json({ ok: true, uptime: process.uptime() });
});
EOF
  step "Update(src/server.ts)  +6 −1";   spin 6 "Пишу тест…"
  mkdir -p test
  cat > test/health.test.ts <<'EOF'
import request from 'supertest';
import { app } from '../src/server';

test('GET /health returns ok', async () => {
  const res = await request(app).get('/health');
  expect(res.status).toBe(200);
  expect(res.body.ok).toBe(true);
});
EOF
  step "Write(test/health.test.ts)";     spin 7 "Запускаю npm test…"
  step "Bash(npm test)"; printf '  %s✓%s 12 passed\n' "$GREEN" "$R"
  ;;
web)
  step "Read(src/Login.tsx)";            spin 8 "Разбираю форму входа…"
  sed -i 's/const \[error, setError\] = useState<string | null>(null);/const [error, setError] = useState<string | null>(null);\n  const [loading, setLoading] = useState(false);/' src/Login.tsx
  cat >> src/Login.tsx <<'EOF'

export function LoginHint() {
  return <p className="hint">Забыли пароль? Напишите в поддержку.</p>;
}
EOF
  step "Update(src/Login.tsx)  +5 −0";   spin 9 "Добавляю состояние загрузки…"
  cat > src/useLogin.ts <<'EOF'
export function useLogin() {
  // TODO: подключить API
  return { submit: async (_email: string, _password: string) => true };
}
EOF
  step "Write(src/useLogin.ts)";         spin 11 "Проверяю типы…"
  step "Bash(npx tsc --noEmit)"; printf '  %s✓%s no errors\n' "$GREEN" "$R"
  ;;
docs)
  step "Read(README.md)";                spin 7 "Читаю структуру README…"
  cat >> README.md <<'EOF'

## Быстрый старт

```sh
radar ~/work/api claude
```
EOF
  step "Update(README.md)  +5 −0"
  # запрос разрешения: событие хука + меню, как у Claude Code
  hook PermissionRequest '{"tool_name":"Bash","tool_input":{"command":"rm -rf dist"}}'
  OPT=("1. Yes" "2. Yes, and don't ask again for rm commands" "3. No, and tell the agent what to do differently (esc)")
  sel=0
  printf '\n  %sBash command%s\n    rm -rf dist\n\n  Do you want to proceed?\n' "$BOLD" "$R"
  while :; do
    for i in 0 1 2; do
      printf '\r\033[K'
      if [ "$i" -eq "$sel" ]; then printf ' %s❯ %s%s' "$BLUE" "${OPT[i]}" "$R"; else printf '   %s' "${OPT[i]}"; fi
      printf '\n'
    done
    IFS= read -rsn1 k
    case "$k" in
      $'\033') IFS= read -rsn2 -t 0.05 rest
               case "$rest" in '[A' | 'OA') ((sel > 0)) && ((sel--));; '[B' | 'OB') ((sel < 2)) && ((sel++));; esac ;;
      '') break ;;
      1 | 2 | 3) sel=$((k - 1)); break ;;
    esac
    printf '\033[3A'
  done
  hook PostToolUse
  spin 8 "Пересобираю документацию…"
  step "Bash(npm run build:docs)"; printf '  %s✓%s done\n' "$GREEN" "$R"
  ;;
esac

printf '\n%s✓%s Готово.\n\n> ' "$GREEN" "$R"
hook Stop
while IFS= read -r _; do :; done
