# Makefile for codeflow CLI
# Build, test, and development targets

BINARY_NAME := codeflow
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
BUILD_DIR := bin
REPO_ROOT := $(shell cd .. && pwd)

# Build flags
LDFLAGS := -X main.version=$(VERSION)
GO_BUILD := CGO_ENABLED=0 go build -ldflags "$(LDFLAGS)"

# Cross-compilation targets
PLATFORMS := linux/amd64 linux/arm64 darwin/amd64 darwin/arm64 windows/amd64

.PHONY: build build-all test test-cover test-race lint copy-schema clean

## build: Build the codeflow binary for the current platform
build:
	$(GO_BUILD) -o $(BUILD_DIR)/$(BINARY_NAME) ./cmd/codeflow/

## build-all: Cross-compile for all supported platforms
build-all:
	@for platform in $(PLATFORMS); do \
		GOOS=$$(echo $$platform | cut -d/ -f1); \
		GOARCH=$$(echo $$platform | cut -d/ -f2); \
		output=$(BUILD_DIR)/$(BINARY_NAME)-$$GOOS-$$GOARCH; \
		if [ "$$GOOS" = "windows" ]; then output=$$output.exe; fi; \
		echo "Building $$platform -> $$output"; \
		CGO_ENABLED=0 GOOS=$$GOOS GOARCH=$$GOARCH go build -ldflags "$(LDFLAGS)" -o $$output ./cmd/codeflow/; \
	done

## test: Run all tests
test:
	go test ./...

## test-cover: Run tests with coverage and enforce 85% threshold on business packages
## Business packages: db, session, cmd/codeflow. Skeleton doc.go-only packages excluded.
test-cover:
	@go test -coverprofile=coverage.out ./... 2>&1 | tee coverage.log
	@echo "--- Per-function coverage ---"
	@go tool cover -func=coverage.out
	@echo "---"
	@total=$$(go tool cover -func=coverage.out | grep ^total: | awk '{print $$NF}' | tr -d '%'); \
	echo "Overall coverage (all packages): $${total}%"
	@go test -coverprofile=business.out \
		./internal/db/... \
		./internal/session/... \
		./cmd/codeflow/... \
		> /dev/null 2>&1; \
	impl=$$(go tool cover -func=business.out | grep ^total: | awk '{print $$NF}' | tr -d '%'); \
	rm -f business.out; \
	echo "Business package coverage: $${impl}%"; \
	threshold=85; \
	if [ $$(echo "$${impl} < $${threshold}" | bc -l) -eq 1 ]; then \
		echo "FAIL: Business package coverage $${impl}% is below $${threshold}% threshold"; \
		exit 1; \
	else \
		echo "OK: Business package coverage $${impl}% meets $${threshold}% threshold"; \
	fi

## test-race: Run tests with race detector
test-race:
	go test -race ./...

## lint: Run golangci-lint
lint:
	golangci-lint run

## copy-schema: Copy DB schema and migrations from project root
copy-schema:
	@mkdir -p internal/db/migrations
	cp $(REPO_ROOT)/.codeflow/scripts/db/schema.sql internal/db/schema.sql
	cp $(REPO_ROOT)/.codeflow/scripts/db/migrations/*.sql internal/db/migrations/

## clean: Remove build artifacts
clean:
	rm -rf $(BUILD_DIR)/
	rm -f coverage.out coverage.log
