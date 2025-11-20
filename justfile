set dotenv-path := "."
set dotenv-override

alias c := check
alias r := run
alias rr := run-release

default:
    @just --list

dev: check fmt lint
    @echo "✅ All checks passed!"

check package='ferrex-player':
    cargo check -p {{package}}

fmt package='ferrex-player':
    cargo fmt -p {{package}}
    cargo clippy -p {{package}} --fix --allow-dirty --allow-staged

lint package='ferrex-player':
    cargo clippy -p {{package}} -- -D warnings

run package='ferrex-player':
    cargo run -p {{package}}

run-release package='ferrex-player':
    cargo run --release -p {{package}}

status:
    @echo "=== Project Status ==="
    @grep -E "^- \[ \]" TODO.md | wc -l | xargs echo "Open tasks:"
    @grep -E "^- \[x\]" TODO.md | wc -l | xargs echo "Completed tasks:"
    @echo ""
    @echo "=== Next 3 Tasks ==="
    @grep -E "^- \[ \]" TODO.md | head -3

task DESCRIPTION:
    echo "- [ ] {{DESCRIPTION}}" >> TODO.md
    @echo "Added: {{DESCRIPTION}}"

today:
    @echo "=== Today's Progress ==="
    @grep -E "^- \[x\].*\($(date +%Y-%m-%d)\)" TODO.md || echo "No tasks completed today yet"
    @echo ""
    @echo "=== Focus Tasks ==="
    @grep -E "^- \[ \].*\*\*.*\*\*" TODO.md || echo "No high-priority tasks"


# Server Testing

mock := env('MOCK_DATABASE_URL')

[working-directory: './ferrex-core']
reset-mock $DATABASE_URL=mock:
    cargo sqlx database reset
    

scan-test: (check 'ferrex-server') reset-mock
    cargo test -p ferrex-server --tests scan
