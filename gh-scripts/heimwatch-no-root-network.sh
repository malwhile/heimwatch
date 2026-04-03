#!/bin/bash
#
# heimwatch-no-root-issues.sh
# Creates labels and issues for implementing the "No-Root" network monitoring feature.
#
# Usage:
#   ./heimwatch-no-root-issues.sh [owner/repo]
#   Example: ./heimwatch-no-root-issues.sh yourusername/heimwatch
#

set -e

PROJECT="git@github.com:malwhile/heimwatch.git"

echo "🚀 Creating labels and issues for No-Root Network Monitoring in ${PROJECT}..."
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
# STEP 1: CREATE TAGS (LABELS)
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 1: Creating Tags (Labels)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Feature Specific Labels
create_label "feature:no-root" "0E8A16" "Feature request for non-privileged operation"
create_label "network-metadata" "0366D6" "Tracking connection metadata (IP, Duration, DNS)"
create_label "procfs" "DEA765" "Implementation using /proc filesystem"
create_label "privacy" "D73A4A" "Privacy-focused implementation (no root)"

# Component Labels (Ensure these exist, create if missing)
create_label "monitoring" "0366D6" "Data collection and metrics"
create_label "cli" "0366D6" "Command line interface"
create_label "architecture" "0366D6" "System design and patterns"

# ============================================================================
# STEP 2: CREATE ISSUES
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 2: Creating Issues"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Issue 1: Research & Socket Discovery
create_issue "🔍 Research: Socket discovery via /proc without root" \
"Research and implement the logic to discover open network sockets for all processes without requiring root privileges.

## Goals
- Identify PIDs with open network sockets by scanning /proc/[pid]/fd/.
- Map socket inodes to network details by reading /proc/net/tcp, /proc/net/udp, /proc/net/tcp6, /proc/net/udp6.
- Verify that this approach works on standard user permissions.

## Technical Details
- Use the procfs crate or standard std::fs to read files.
- Parse the hex-encoded addresses in /proc/net/* (e.g., 0100007F:1F90 -> 127.0.0.1:8000).
- Map the inode number found in /proc/[pid]/fd/ to the corresponding line in /proc/net/*.

## Deliverables
- A Rust module socket_scanner.rs in heimwatch-collector.
- Function scan_sockets() returning Vec<SocketInfo> that returns PID, RemoteIP, RemotePort, State, Inode.
- Unit tests mocking /proc filesystem behavior.

## Acceptance Criteria
- [ ] Code runs without sudo.
- [ ] Successfully identifies established connections for known processes (e.g., Firefox, Chrome).
- [ ] Handles IPv4 and IPv6.
- [ ] Documentation added to /docs/research/no-root-network.md" \
"feature:no-root,network-metadata,procfs,research,linux" \
"Phase 2: Data Collection Core"

# Issue 2: Data Extraction & Database Integration
create_issue "💾 Extract connection metadata and store in Sled" \
"Implement the logic to extract meaningful metadata from the raw socket data and persist it to the database.

## Goals
- Convert raw socket data into ConnectionRecord structs.
- Calculate connection duration by comparing current scan with previous scan (delta tracking).
- Perform reverse DNS lookups (optional, async) to get hostnames.
- Integrate with heimwatch-storage to save records.

## Data to Store
- PID
- Remote IP
- Remote Port
- Hostname (resolved via DNS)
- Connection State (ESTABLISHED, TIME_WAIT, etc.)
- Start Time (first seen)
- Last Seen Time
- Total Duration (calculated)

## Technical Details
- Use trust-dns-resolver or std::net for DNS lookups (handle timeouts gracefully).
- Use maxminddb crate for GeoIP lookup (optional, local DB).
- Store records with a prefix conn: in Sled.
- Handle PID reuse: If a PID reappears with a new connection, reset the start time.

## Acceptance Criteria
- [ ] ConnectionRecord model defined in heimwatch-core.
- [ ] store_connection() method in heimwatch-storage.
- [ ] Background task runs every 5s, scans, calculates duration, and saves.
- [ ] No data loss if DNS lookup fails (store IP as fallback).
- [ ] Database queries work for Top Connected Hosts and Longest Connections." \
"feature:no-root,network-metadata,procfs,implementation,storage" \
"Phase 2: Data Collection Core"

# Issue 3: Dual-Mode Architecture (Root vs. Non-Root)
create_issue "🏗️ Refactor network collector to support Root and Non-Root modes" \
"Refactor the network collection logic to support both high-precision (root/eBPF) and metadata-only (non-root/procfs) modes.

## Goals
- Create a NetworkCollector trait with two implementations:
  1. RootCollector: Uses eBPF/libpcap for byte counts (requires root).
  2. UserCollector: Uses /proc for metadata only (no root).
- Implement a factory pattern to select the correct collector based on capabilities or flags.

## Technical Details
- Detect root privileges at runtime using libc::geteuid().
- If running as root, prefer RootCollector but allow override.
- If running as user, force UserCollector.
- Ensure the MetricRecord struct can handle optional fields (e.g., bytes_tx: Option<u64>).

## Deliverables
- heimwatch-collector/src/network/mod.rs with trait and implementations.
- Logic to switch modes dynamically or at startup.
- Logging: Warn user if running in Limited Mode (no root).

## Acceptance Criteria
- [ ] Binary compiles and runs as root (full mode).
- [ ] Binary compiles and runs as user (limited mode).
- [ ] Web/TUI interfaces correctly display Limited Mode warning if applicable.
- [ ] No panic if eBPF fails to load when running as user." \
"feature:no-root,architecture,implementation,monitoring" \
"Phase 2: Data Collection Core"

# Issue 4: CLI Argument for Non-Root Mode
create_issue "⚙️ Add CLI flag to force non-root network monitoring" \
"Add a command-line argument to explicitly force the daemon to run in non-root mode, even if run as root.

## Goals
- Add --no-root or --user-mode flag to the CLI.
- Override automatic privilege detection if this flag is present.
- Update help text to explain the difference between modes.

## CLI Changes
- heimwatch start --no-root: Forces metadata-only collection.
- heimwatch start: Auto-detects (Root -> Full, User -> Limited).

## Technical Details
- Update clap argument definitions in heimwatch-daemon.
- Pass the flag to the NetworkCollector factory.
- Update README to document the flag.

## Acceptance Criteria
- [ ] --help shows the new flag.
- [ ] Running heimwatch start --no-root as root disables byte counting.
- [ ] Running heimwatch start as user automatically uses limited mode.
- [ ] Console output clearly states: Running in User Mode: Network volume tracking disabled.
- [ ] Tests verify flag parsing and mode selection." \
"feature:no-root,cli,implementation" \
"Phase 2: Data Collection Core"

# ============================================================================
# FINAL SUMMARY
# ============================================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ No-Root Feature Issues Created!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Created labels:"
echo "  - feature:no-root"
echo "  - network-metadata"
echo "  - procfs"
echo "  - privacy"
echo ""
echo "Created issues:"
echo "  1. 🔍 Research: Socket discovery via /proc without root"
echo "  2. 💾 Extract connection metadata and store in Sled"
echo "  3. 🏗️ Refactor network collector to support Root and Non-Root modes"
echo "  4. ⚙️ Add CLI flag to force non-root network monitoring"
echo ""
echo "Next steps:"
echo "  1. Review issues at: https://github.com/${PROJECT}/issues"
echo "  2. Assign to Phase 2: Data Collection Core milestone."
echo "  3. Start with Issue #1 (Research)."
echo ""
