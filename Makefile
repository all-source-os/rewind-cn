# =============================================================================
# Rewind — Build & Release
# =============================================================================

GITHUB_ORG := all-source-os
REPO_NAME := ralph-allsource

# =============================================================================
# Development
# =============================================================================

.PHONY: build test clippy fmt check ci

build:
	cargo build

test:
	cargo test --all

clippy:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

check: fmt-check clippy test

ci: check
	@echo ""
	@echo "=== All quality gates passed ==="

# =============================================================================
# Release
# =============================================================================

.PHONY: release release-quick release-preflight version set-version bump-version

release: release-preflight
	@echo ""
	@echo "=== Release Configuration ==="
	@echo ""
	@CURRENT=$$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0"); \
	echo "Current version: $$CURRENT"; \
	echo ""; \
	MAJOR=$$(echo $$CURRENT | sed 's/v//' | cut -d. -f1); \
	MINOR=$$(echo $$CURRENT | sed 's/v//' | cut -d. -f2); \
	PATCH=$$(echo $$CURRENT | sed 's/v//' | cut -d. -f3); \
	NEXT_PATCH="v$${MAJOR}.$${MINOR}.$$((PATCH + 1))"; \
	NEXT_MINOR="v$${MAJOR}.$$((MINOR + 1)).0"; \
	NEXT_MAJOR="v$$((MAJOR + 1)).0.0"; \
	echo "Version suggestions:"; \
	echo "  1) $$NEXT_PATCH (patch - bug fixes)"; \
	echo "  2) $$NEXT_MINOR (minor - new features)"; \
	echo "  3) $$NEXT_MAJOR (major - breaking changes)"; \
	echo "  4) Custom version"; \
	echo ""; \
	read -p "Select version type (1-4) [1]: " VERSION_TYPE; \
	VERSION_TYPE=$${VERSION_TYPE:-1}; \
	case $$VERSION_TYPE in \
		1) VERSION=$$NEXT_PATCH ;; \
		2) VERSION=$$NEXT_MINOR ;; \
		3) VERSION=$$NEXT_MAJOR ;; \
		4) read -p "Enter version (e.g., v1.0.0): " VERSION ;; \
		*) VERSION=$$NEXT_PATCH ;; \
	esac; \
	if ! echo "$$VERSION" | grep -qE '^v[0-9]+\.[0-9]+\.[0-9]+$$'; then \
		echo "ERROR: Invalid version format. Use vX.Y.Z"; \
		exit 1; \
	fi; \
	if git tag -l | grep -q "^$${VERSION}$$"; then \
		echo "ERROR: Tag $$VERSION already exists!"; \
		exit 1; \
	fi; \
	echo ""; \
	read -p "Release title (e.g., 'Chronis Integration'): " TITLE; \
	TITLE=$${TITLE:-"Release"}; \
	echo ""; \
	read -p "Run quality gates before release? (Y/n): " RUN_QG; \
	if [ "$$RUN_QG" != "n" ] && [ "$$RUN_QG" != "N" ]; then \
		$(MAKE) ci || exit 1; \
	fi; \
	echo ""; \
	echo "=== Updating Version ==="; \
	VER_NUM=$$(echo $$VERSION | sed 's/v//'); \
	$(MAKE) set-version VERSION=$$VER_NUM; \
	git add Cargo.toml Cargo.lock CHANGELOG.md; \
	git commit -m "release: $$VERSION" || true; \
	echo ""; \
	echo "=== Creating Git Tag ==="; \
	CHANGES=$$(git log $$(git describe --tags --abbrev=0 2>/dev/null || echo "HEAD~10")..HEAD --oneline | head -15); \
	echo "Recent changes:"; \
	echo "$$CHANGES"; \
	echo ""; \
	read -p "Create tag $$VERSION and push? (Y/n): " CONFIRM; \
	if [ "$$CONFIRM" = "n" ] || [ "$$CONFIRM" = "N" ]; then \
		echo "Aborted."; \
		exit 1; \
	fi; \
	git tag -a "$$VERSION" -m "$$VERSION - $$TITLE"; \
	echo ""; \
	echo "=== Pushing to Remote ==="; \
	git push origin main; \
	git push origin "$$VERSION"; \
	echo ""; \
	echo "=== Creating GitHub Release ==="; \
	PREV_TAG=$$(git describe --tags --abbrev=0 $$VERSION^ 2>/dev/null || echo ""); \
	gh release create "$$VERSION" \
		--title "Rewind $$VERSION - $$TITLE" \
		--generate-notes; \
	echo ""; \
	echo "========================================="; \
	echo "  Release $$VERSION Complete!"; \
	echo "========================================="; \
	echo ""; \
	echo "Artifacts:"; \
	echo "  - Git tag: $$VERSION"; \
	echo "  - GitHub Release: https://github.com/$(GITHUB_ORG)/$(REPO_NAME)/releases/tag/$$VERSION"; \
	echo ""; \
	echo "Binary artifacts will be available once the release workflow completes."; \
	echo "  Monitor at: https://github.com/$(GITHUB_ORG)/$(REPO_NAME)/actions"

release-quick: release-preflight
	@echo ""
	@echo "=== Quick Release (skipping quality gates) ==="
	@CURRENT=$$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0"); \
	MAJOR=$$(echo $$CURRENT | sed 's/v//' | cut -d. -f1); \
	MINOR=$$(echo $$CURRENT | sed 's/v//' | cut -d. -f2); \
	PATCH=$$(echo $$CURRENT | sed 's/v//' | cut -d. -f3); \
	VERSION="v$${MAJOR}.$${MINOR}.$$((PATCH + 1))"; \
	echo "Creating patch release: $$VERSION"; \
	VER_NUM=$$(echo $$VERSION | sed 's/v//'); \
	$(MAKE) set-version VERSION=$$VER_NUM; \
	git add Cargo.toml Cargo.lock CHANGELOG.md; \
	git commit -m "release: $$VERSION" || true; \
	git tag -a "$$VERSION" -m "$$VERSION - Patch Release"; \
	git push origin main; \
	git push origin "$$VERSION"; \
	gh release create "$$VERSION" --title "Rewind $$VERSION" --generate-notes; \
	echo ""; \
	echo "Release $$VERSION created!"

release-preflight:
	@echo "=== Release Pre-flight Checks ==="
	@echo ""
	@if [ -n "$$(git status --porcelain)" ]; then \
		echo "ERROR: You have uncommitted changes:"; \
		git status --short; \
		exit 1; \
	fi
	@BRANCH=$$(git branch --show-current); \
	if [ "$$BRANCH" != "main" ]; then \
		echo "WARNING: You're on branch '$$BRANCH', not 'main'"; \
		read -p "Continue anyway? (y/N): " REPLY; \
		if [ "$$REPLY" != "y" ] && [ "$$REPLY" != "Y" ]; then \
			exit 1; \
		fi; \
	fi
	@if ! git ls-remote --exit-code origin &>/dev/null; then \
		echo "ERROR: Cannot reach git remote 'origin'"; \
		exit 1; \
	fi
	@if ! gh auth status &>/dev/null; then \
		echo "ERROR: GitHub CLI not authenticated. Run 'gh auth login'"; \
		exit 1; \
	fi
	@echo "Pre-flight checks passed!"

version:
	@echo "Current version: $$(git describe --tags --abbrev=0 2>/dev/null || echo 'no tags')"
	@echo "Cargo.toml version: $$(grep '^version' Cargo.toml | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
	@echo ""
	@echo "Recent commits:"
	@git log --oneline -5
	@echo ""
	@echo "Tags:"
	@git tag -l | sort -V | tail -5

# Usage: make set-version VERSION=0.2.0
set-version:
ifndef VERSION
	@echo "ERROR: VERSION is required. Usage: make set-version VERSION=0.2.0"
	@exit 1
endif
	@echo "Setting version to $(VERSION)..."
	@sed -i '' 's/^version = "[0-9]*\.[0-9]*\.[0-9]*"/version = "$(VERSION)"/' Cargo.toml
	@cargo check --quiet 2>/dev/null || true
	@echo "Updated:"
	@echo "  - Cargo.toml (workspace version)"
	@echo "  - Cargo.lock (via cargo check)"

bump-version:
	@echo "=== Interactive Version Bump ==="
	@CURRENT=$$(grep '^version' Cargo.toml | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+'); \
	echo "Current version: $$CURRENT"; \
	echo ""; \
	MAJOR=$$(echo $$CURRENT | cut -d. -f1); \
	MINOR=$$(echo $$CURRENT | cut -d. -f2); \
	PATCH=$$(echo $$CURRENT | cut -d. -f3); \
	NEXT_PATCH="$${MAJOR}.$${MINOR}.$$((PATCH + 1))"; \
	NEXT_MINOR="$${MAJOR}.$$((MINOR + 1)).0"; \
	NEXT_MAJOR="$$((MAJOR + 1)).0.0"; \
	echo "Select new version:"; \
	echo "  1) $$NEXT_PATCH (patch)"; \
	echo "  2) $$NEXT_MINOR (minor)"; \
	echo "  3) $$NEXT_MAJOR (major)"; \
	echo "  4) Custom"; \
	echo ""; \
	read -p "Choice [1]: " CHOICE; \
	CHOICE=$${CHOICE:-1}; \
	case $$CHOICE in \
		1) NEW_VERSION=$$NEXT_PATCH ;; \
		2) NEW_VERSION=$$NEXT_MINOR ;; \
		3) NEW_VERSION=$$NEXT_MAJOR ;; \
		4) read -p "Enter version: " NEW_VERSION ;; \
		*) NEW_VERSION=$$NEXT_PATCH ;; \
	esac; \
	echo ""; \
	read -p "Set version to $$NEW_VERSION? (Y/n): " CONFIRM; \
	if [ "$$CONFIRM" != "n" ] && [ "$$CONFIRM" != "N" ]; then \
		$(MAKE) set-version VERSION=$$NEW_VERSION; \
	else \
		echo "Aborted."; \
	fi

.PHONY: help
help:
	@echo "Development:"
	@echo "  make build          Build the project"
	@echo "  make test           Run all tests"
	@echo "  make clippy         Run clippy lints"
	@echo "  make fmt            Format code"
	@echo "  make check          Run all quality gates (fmt, clippy, test)"
	@echo "  make ci             Alias for check"
	@echo ""
	@echo "Release:"
	@echo "  make release        Full interactive release workflow"
	@echo "  make release-quick  Quick patch release (skip quality gates)"
	@echo "  make version        Show current version and recent history"
	@echo "  make set-version    Set version: make set-version VERSION=0.2.0"
	@echo "  make bump-version   Interactive version bump"
