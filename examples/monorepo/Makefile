# Root Makefile of an app with a Rust server, an admin CLI and a frontend.
include .env
export

.PHONY: dev build test frontend-build db-migrate db-reset db-seed create-user

dev: build
	./target/debug/server --port $${PORT:-3000}

build: frontend-build
	cargo build

test: frontend-build
	cargo test
	$(MAKE) -C frontend test

frontend-build:
	$(MAKE) -C frontend build

db-migrate:
	sqlx migrate run --database-url "$(DATABASE_URL)"

db-reset:
	@if [ "$(APP_ENV)" = "prod" ]; then echo "refusing to run in prod"; exit 1; fi
	@read -p "This wipes the local database. Continue? [y/N] " a && [ "$$a" = y ]
	sqlx database reset -y --database-url "$(DATABASE_URL)"
	$(MAKE) db-seed

db-seed:
	psql "$(DATABASE_URL)" -f db/seed.sql

# usage: make create-user NAME=bas ROLE=admin
create-user: build
	@test -n "$(NAME)" || (echo "usage: make create-user NAME=... [ROLE=...]"; exit 1)
	./target/debug/admin create-user "$(NAME)" --role "$${ROLE:-user}"
