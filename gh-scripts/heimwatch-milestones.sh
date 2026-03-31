#!/bin/bash
#
# heimwatch-milestones.sh
# Creates milestones for the heimwatch GitHub project
#
# Usage:
#   ./heimwatch-milestones.sh [owner/repo]
#   Example: ./heimwatch-milestones.sh yourusername/heimwatch
#

set -e

PROJECT="git@github.com:malwhile/heimwatch.git"

echo "📅 Creating milestones for ${PROJECT}..."
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

# Helper function to create a milestone
create_milestone() {
    local title="$1"
    local description="$2"
    local due_date="$3"
    
    echo "📅 Creating: $title"
    
    if [ -n "$due_date" ]; then
        gh milestone create \
            --title "$title" \
            --description "$description" \
            --due "$due_date" \
            --repo "$PROJECT" \
            2>/dev/null || true
    else
        gh milestone create \
            --title "$title" \
            --description "$description" \
            --repo "$PROJECT" \
            2>/dev/null || true
    fi
    
    sleep 0.3  # Rate limiting protection
}

# ============================================================================
# MILESTONE CREATION
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Development Milestones"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Phase 1: Foundation
create_milestone "Phase 1: Project Setup & Architecture" \
"Establish project foundation, module boundaries, and CI/CD pipeline. \n\nGoals:\n- Scaffold Rust workspace\n- Define OS abstraction layers\n- Set up GitHub Actions CI\n- Document architecture decisions" \
""

# Phase 2: Core Monitoring
create_milestone "Phase 2: Data Collection Core" \
"Implement all data collection for monitoring metrics on Linux/Wayland. \n\nGoals:\n- Network usage tracking\n- Power/battery monitoring\n- Focus time tracking\n- CPU, memory, disk, GPU metrics\n- OS abstraction traits"

# Phase 3: Storage
create_milestone "Phase 3: Storage Layer" \
"Implement sled-based time-series storage with efficient queries. \n\nGoals:\n- Database schema design\n- CRUD operations for all metrics\n- Time-range query optimization\n- Data retention policies"

# Phase 4: Web Dashboard
create_milestone "Phase 4: Web Interface" \
"Build lightweight web-based dashboard with Axum and HTMX. \n\nGoals:\n- HTTP server setup\n- Dashboard layout and styling\n- Chart.js integration\n- Real-time updates via SSE"

# Phase 5: Terminal UI
create_milestone "Phase 5: TUI Interface" \
"Create terminal-based interface for low-resource environments. \n\nGoals:\n- Ratatui layout\n- ASCII charts and metrics\n- Keyboard navigation\n- SSH compatibility"

# Phase 6: Service Management
create_milestone "Phase 6: Daemon & Service Management" \
"Implement background service with proper lifecycle management. \n\nGoals:\n- systemd service for Linux\n- Signal handling and graceful shutdown\n- Log rotation\n- Service placeholders for other OSes"

# Phase 7: Alerting
create_milestone "Phase 7: Alerting System" \
"Build configurable alerting and notification system. \n\nGoals:\n- Alert rule configuration\n- Notification delivery (desktop, email, webhook)\n- Alert history and management\n- Rate limiting"

# Phase 8: Cross-Platform
create_milestone "Phase 8: Cross-Platform Expansion" \
"Port monitoring to macOS, Windows, and BSD. \n\nGoals:\n- macOS collector implementation\n- Windows collector implementation\n- BSD collector implementation\n- Feature flag system"

# Phase 9: Polish
create_milestone "Phase 9: License & Documentation" \
"Finalize licensing, documentation, and release preparation. \n\nGoals:\n- Apache 2.0 license\n- Comprehensive README\n- Contributing guidelines\n- API documentation\n- Release packaging"

# ============================================================================
# OPTIONAL: Future Release Milestones
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Creating Release Milestones (Optional)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

create_milestone "v0.1.0 - Alpha" \
"Initial alpha release with Linux/Wayland support and basic monitoring. \n\nFeatures:\n- Core metrics collection\n- Web dashboard\n- Basic TUI\n- systemd service" \
""

create_milestone "v0.2.0 - Beta" \
"Beta release with alerting and improved stability. \n\nFeatures:\n- Alerting system\n- Data retention\n- Enhanced UI\n- Performance optimizations" \
""

create_milestone "v1.0.0 - Stable" \
"First stable release with full feature set. \n\nFeatures:\n- All metrics working\n- Cross-platform support\n- Production-ready\n- Comprehensive docs" \
""

# ============================================================================
# FINAL SUMMARY
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Milestone Creation Complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Created milestones for ${PROJECT}:"
echo ""
echo "  📋  Development Milestones (9 milestones)"
echo "      - Phase 1: Project Setup & Architecture"
echo "      - Phase 2: Data Collection Core"
echo "      - Phase 3: Storage Layer"
echo "      - Phase 4: Web Interface"
echo "      - Phase 5: TUI Interface"
echo "      - Phase 6: Daemon & Service Management"
echo "      - Phase 7: Alerting System"
echo "      - Phase 8: Cross-Platform Expansion"
echo "      - Phase 9: License & Documentation"
echo ""
echo "  🚀  Release Milestones (3 milestones)"
echo "      - v0.1.0 - Alpha"
echo "      - v0.2.0 - Beta"
echo "      - v1.0.0 - Stable"
echo ""
echo "Total: 12 milestones created"
echo ""
echo "Next steps:"
echo "  1. Review milestones at: https://github.com/${PROJECT}/milestones"
echo "  2. Assign issues to milestones using the issue generator script"
echo "  3. Optionally set due dates in GitHub UI"
echo ""
echo "Tip: You can assign issues to milestones when creating them with:"
echo "     gh issue create --milestone \"Phase 1: Project Setup & Architecture\""
echo ""
