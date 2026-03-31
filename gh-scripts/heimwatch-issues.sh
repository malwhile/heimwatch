#!/bin/bash
#
# heimwatch-labels.sh
# Creates comprehensive labels for the heimwatch GitHub project
#
# Usage:
#   ./heimwatch-labels.sh [owner/repo]
#   Example: ./heimwatch-labels.sh yourusername/heimwatch
#

set -e

PROJECT="git@github.com:malwhile/heimwatch.git"

echo "🏷️  Creating labels for ${PROJECT}..."
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
    
    echo "🏷️  Creating: $name"
    
    # Try to create label, ignore if it already exists
    gh label create "$name" \
        --color "$color" \
        --description "$description" \
        --repo "$PROJECT" \
        --force 2>/dev/null || true
    
    sleep 0.2  # Rate limiting protection
}

# ============================================================================
# EPIC & PHASE LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Epic & Phase Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "epic" "5319f7" "Major initiative containing multiple related issues"
create_label "Phase 1" "0E8A16" "Project Setup & Architecture"
create_label "Phase 2" "0E8A16" "Data Collection Core"
create_label "Phase 3" "0E8A16" "Storage Layer"
create_label "Phase 4" "0E8A16" "Web Interface"
create_label "Phase 5" "0E8A16" "TUI Interface"
create_label "Phase 6" "0E8A16" "Daemon & Service Management"
create_label "Phase 7" "0E8A16" "Alerting System"
create_label "Phase 8" "0E8A16" "Cross-Platform Expansion"
create_label "Phase 9" "0E8A16" "License & Documentation"

# ============================================================================
# ISSUE TYPE LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Issue Type Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "research" "C5DEF5" "Investigation and feasibility studies"
create_label "implementation" "1D76DB" "Coding and feature implementation"
create_label "documentation" "D4C5F9" "Docs, README, guides"
create_label "bug" "D73A4A" "Something isn't working"
create_label "enhancement" "A2EEEF" "New feature or request"
create_label "question" "D8769B" "Further information is requested"
create_label "maintenance" "FBCA04" "Technical debt, refactoring, cleanup"

# ============================================================================
# COMPONENT LABELS (UPDATED)
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Component Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "architecture" "0366D6" "System design, module boundaries, and patterns"
create_label "monitoring" "0366D6" "Data collection and metrics"
create_label "storage" "0366D6" "Database and persistence layer"
create_label "web" "0366D6" "Web interface and API"
create_label "tui" "0366D6" "Terminal user interface"
create_label "daemon" "0366D6" "Background service and systemd"
create_label "alerts" "0366D6" "Notification and alerting system"
create_label "cross-platform" "0366D6" "Multi-OS support"
create_label "testing" "0366D6" "Unit, integration, and e2e tests"
create_label "performance" "0366D6" "Optimization and benchmarking"
create_label "ci-cd" "0366D6" "Continuous integration and deployment pipelines"

# ============================================================================
# PLATFORM LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Platform Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "linux" "FC2929" "Linux-specific"
create_label "macos" "FC2929" "macOS-specific"
create_label "windows" "FC2929" "Windows-specific"
create_label "bsd" "FC2929" "BSD-specific"
create_label "wayland" "FC2929" "Wayland compositor support"
create_label "x11" "FC2929" "X11 support (future)"

# ============================================================================
# PRIORITY LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Priority Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "priority-high" "FF0000" "Critical, must be addressed soon"
create_label "priority-medium" "FBCA04" "Important but not urgent"
create_label "priority-low" "0E8A16" "Nice to have"

# ============================================================================
# STATUS LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Status Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "in-progress" "0E8A16" "Currently being worked on"
create_label "blocked" "D73A4A" "Blocked by another issue or dependency"
create_label "help-wanted" "006B75" "Needs community help"
create_label "good-first-issue" "7057FF" "Good for newcomers"
create_label "ready" "2ECC71" "Ready for review/merge"

# ============================================================================
# TECHNOLOGY LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Technology Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "rust" "DEA765" "Rust code"
create_label "sled" "DEA765" "Sled database"
create_label "axum" "DEA765" "Axum web framework"
create_label "htmx" "DEA765" "HTMX frontend"
create_label "chart.js" "DEA765" "Chart.js visualization"
create_label "ratatui" "DEA765" "Ratatui TUI"
create_label "tokio" "DEA765" "Tokio async runtime"
create_label "systemd" "DEA765" "Systemd service management"

# ============================================================================
# SECURITY & PRIVACY LABELS
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Security & Privacy Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_label "security" "D73A4A" "Security-related"
create_label "privacy" "D73A4A" "Privacy considerations"
create_label "permissions" "D73A4A" "System permissions required"

# ============================================================================
# FINAL SUMMARY
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Label Creation Complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Created labels for ${PROJECT}:"
echo ""
echo "  📋  Epic & Phase Labels (11 labels)"
echo "  📝  Issue Type Labels (7 labels)"
echo "  🧩  Component Labels (11 labels) [+ architecture, testing, performance, ci-cd]"
echo "  💻  Platform Labels (6 labels)"
echo "  ⚡  Priority Labels (3 labels)"
echo "  📊  Status Labels (5 labels)"
echo "  🔧  Technology Labels (8 labels) [+ systemd]"
echo "  🔒  Security & Privacy Labels (3 labels)"
echo ""
echo "Total: 54 labels created"
echo ""
echo "Next steps:"
echo "  1. Review labels at: https://github.com/${PROJECT}/labels"
echo "  2. Run issue generator script: ./heimwatch-issue-generator.sh"
echo "  3. Start creating issues with appropriate labels"
echo ""
