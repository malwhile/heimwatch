#!/bin/bash
#
# heimwatch-unit-tests-issue.sh
# Creates labels and an issue for adding unit tests to the Linux collector logic
#
# Usage:
#   ./heimwatch-unit-tests-issue.sh [owner/repo]
#   Example: ./heimwatch-unit-tests-issue.sh yourusername/heimwatch
#

set -e

PROJECT="git@github.com:malwhile/heimwatch.git"

echo "🚀 Creating labels and issue for Linux Collector Unit Tests in ${PROJECT}..."
echo ""

# Verify gh is authenticated
if ! gh auth status &>/dev/null; then
    echo "❌ Not authenticated with GitHub. Run: gh auth login"
    exit 1
fi

# Verify repo exists
if ! gh repo view "$PROJECT" &>/dev/null; then
    echo "❌ Repository ${PROJECT} not found or not accessible."
    exit 1
fi

# Helper function to create a label
create_label() {
    local name="$1"
    local color="$2"
    local description="$3"
    
    echo "🏷️  Creating Label: $name"
    gh label create "$name" \
        --color "$color" \
        --description "$description" \
        --repo "$PROJECT" \
        --force 2>/dev/null || true
    sleep 0.2
}

# Helper function to create an issue
create_issue() {
    local title="$1"
    local body="$2"
    local labels="$3"
    local milestone="$4"
    
    echo "📝 Creating Issue: $title"
    
    if [ -n "$milestone" ]; then
        gh issue create \
            --repo "$PROJECT" \
            --title "$title" \
            --body "$body" \
            --label "$labels" \
            --milestone "$milestone"
    else
        gh issue create \
            --repo "$PROJECT" \
            --title "$title" \
            --body "$body" \
            --label "$labels"
    fi
    sleep 0.5
}

# ============================================================================
# STEP 1: CREATE LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 1: Creating/Ensuring Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Ensure these labels exist
create_label "unit-tests" "0366D6" "Unit testing and code coverage"
create_label "linux" "FC2929" "Linux-specific implementation"
create_label "refactor" "0075C9" "Code refactoring and improvement"
create_label "testing" "0366D6" "General testing and QA"

sleep 0.2

# ============================================================================
# STEP 2: CREATE ISSUE
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 2: Creating Issue"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_issue "🧪 Add Unit Tests for Linux Collector Logic (Delta, Aggregation, PID Reuse)" \
"The core logic in `crates/heimwatch-collector/src/linux.rs` (lines 110-132) currently lacks unit tests. Reliance on end-to-end daemon tests makes it difficult to catch edge cases like counter wraparound or PID reuse bugs early.

## Context
File: `crates/heimwatch-collector/src/linux.rs`
Current State:
- Delta calculation, app aggregation, and PID reuse handling are only tested via the full daemon loop.
- No isolated tests for the mathematical logic or state management.

## Goal
Add a `#[cfg(test)]` module to `linux.rs` (or a separate `linux_tests.rs`) with targeted unit tests for the core algorithms.

## Required Test Cases

### 1. Delta Calculation with PID Reuse
- **Scenario:** PID 1234 has 100 bytes. PID 1234 dies and a new process gets PID 1234 with 0 bytes.
- **Expected:** The delta calculation should reset or handle the discontinuity gracefully (no negative numbers).
- **Mock:** Provide `previous_state` and `current_state` maps.

### 2. Aggregation of Multiple PIDs per App
- **Scenario:** Two processes (PID 100, PID 101) both belong to 'firefox'.
- **Expected:** Aggregation logic correctly sums their bytes into a single 'firefox' entry.
- **Mock:** Input map with multiple PIDs, expected output map with one aggregated entry.

### 3. Counter Overflow Handling
- **Scenario:** A counter reaches `u64::MAX` (or close to it) and wraps.
- **Expected:** Logic uses `saturating_add` or handles wraparound correctly to avoid negative deltas.
- **Mock:** Input values near `u64::MAX`.

### 4. Empty/Missing Data Handling
- **Scenario:** A PID exists in `previous` but not `current` (process exited).
- **Expected:** Logic handles the missing key without panicking.

## Implementation Details
- Use `mockall` or simple struct literals for input data.
- Ensure tests run with `cargo test --package heimwatch-collector`.
- Aim for >80% coverage on the `collect_network` and `calculate_deltas` functions.

## Acceptance Criteria
- [ ] `linux.rs` (or dedicated test file) contains a `mod tests` block.
- [ ] At least 4 distinct test cases covering the scenarios above.
- [ ] All tests pass locally and in CI.
- [ ] Code coverage report shows increased coverage for `linux.rs`.
- [ ] No panics in edge cases (PID reuse, overflow).

## Priority
High. Prevents subtle data corruption bugs in production." \
"unit-tests,linux,refactor,testing" \
"Phase 2: Data Collection Core"

# ============================================================================
# FINAL SUMMARY
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Issue Created!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Created labels:"
echo "  - unit-tests"
echo "  - linux"
echo "  - refactor"
echo "  - testing"
echo ""
echo "Created issue:"
echo "  🧪 Add Unit Tests for Linux Collector Logic (Delta, Aggregation, PID Reuse)"
echo ""
echo "Next steps:"
echo "  1. Review issue at: https://github.com/${PROJECT}/issues"
echo "  2. Assign to 'Phase 2: Data Collection Core' milestone."
echo "  3. Implement tests to catch edge cases early."
echo ""
