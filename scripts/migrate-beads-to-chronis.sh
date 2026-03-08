#!/usr/bin/env bash
# Migrate docs/beads/ specs into chronis tasks.
#
# Prerequisites:
#   - cn CLI installed and on PATH
#   - cn init already run in the project root
#
# Usage:
#   ./scripts/migrate-beads-to-chronis.sh [--dry-run]

set -euo pipefail

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
    echo "[dry-run] No tasks will be created."
fi

BEADS_DIR="docs/beads"

if ! command -v cn &>/dev/null; then
    echo "Error: cn (chronis) CLI not found on PATH." >&2
    exit 1
fi

if [[ ! -d "$BEADS_DIR" ]]; then
    echo "Error: $BEADS_DIR directory not found." >&2
    exit 1
fi

# Create the parent epic first
EPIC_TITLE="PRD-002: CLI Commands (Phase 1)"
EPIC_DESC="Migrated from docs/beads/ — US-002 through US-005 implementation specs"

if [[ "$DRY_RUN" == "false" ]]; then
    echo "Creating epic: $EPIC_TITLE"
    EPIC_ID=$(cn task create --type epic --title "$EPIC_TITLE" --description "$EPIC_DESC" --toon 2>/dev/null | grep -oE '[a-f0-9-]+' | head -1)
    echo "  Epic ID: $EPIC_ID"
else
    echo "Would create epic: $EPIC_TITLE"
    EPIC_ID="dry-run-epic"
fi

# Parse each bead file and create a task
for bead_file in "$BEADS_DIR"/*.md; do
    filename=$(basename "$bead_file" .md)

    # Extract title from first H1 line
    title=$(head -1 "$bead_file" | sed 's/^# //')
    if [[ -z "$title" ]]; then
        title="$filename"
    fi

    # Extract the Goal section as description
    desc=$(awk '/^## Goal/{flag=1; next} /^## /{flag=0} flag' "$bead_file" | head -20)
    if [[ -z "$desc" ]]; then
        desc="Bead spec: $filename"
    fi

    # Determine US number for type tagging
    us_num=$(echo "$filename" | grep -oE 'US-[0-9]+' || echo "")

    if [[ "$DRY_RUN" == "false" ]]; then
        echo "Creating task: $title"
        cn task create \
            --title "$title" \
            --description "$desc" \
            --type task \
            --toon 2>/dev/null || echo "  Warning: failed to create $title"
    else
        echo "Would create task: $title ($us_num)"
        echo "  Desc: ${desc:0:80}..."
    fi
done

echo ""
if [[ "$DRY_RUN" == "false" ]]; then
    echo "Migration complete. Run 'cn list --toon' to verify."
else
    echo "[dry-run] complete. Remove --dry-run to execute."
fi
