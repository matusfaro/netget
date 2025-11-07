#!/bin/bash

# lines-of-code.sh - NetGet Codebase Statistics
# Generates comprehensive LOC stats, LOC per day, and per-protocol breakdowns

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}        NetGet Codebase Statistics${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}\n"

# 1. TOTAL LOC
echo -e "${GREEN}📊 TOTAL CODEBASE STATISTICS${NC}"
echo "─────────────────────────────────────────────────────────"

# Count all Rust files
TOTAL_LOC=$(find src tests -type f -name "*.rs" 2>/dev/null | xargs wc -l | tail -1 | awk '{print $1}')
echo -e "Total Lines of Code (Rust):     ${YELLOW}${TOTAL_LOC}${NC}"

# Count files
TOTAL_FILES=$(find src tests -type f -name "*.rs" 2>/dev/null | wc -l)
echo -e "Total Rust Files:               ${YELLOW}${TOTAL_FILES}${NC}"

# 2. TIME ELAPSED
FIRST_COMMIT_DATE=$(git log --format="%ai" | tail -1)
FIRST_COMMIT_EPOCH=$(git log --format="%at" | tail -1)
LAST_COMMIT_DATE=$(git log --format="%ai" | head -1)
LAST_COMMIT_EPOCH=$(git log --format="%at" | head -1)

DAYS_ELAPSED=$(( (LAST_COMMIT_EPOCH - FIRST_COMMIT_EPOCH) / 86400 ))
# Handle case where commits are on same day
if [ $DAYS_ELAPSED -eq 0 ]; then
    DAYS_ELAPSED=1
fi

echo -e "First Commit:                   ${YELLOW}${FIRST_COMMIT_DATE}${NC}"
echo -e "Last Commit:                    ${YELLOW}${LAST_COMMIT_DATE}${NC}"
echo -e "Days Since Project Start:       ${YELLOW}${DAYS_ELAPSED}${NC}"

COMMITS=$(git log --oneline | wc -l)
echo -e "Total Commits:                  ${YELLOW}${COMMITS}${NC}"

# 3. LOC PER DAY
LOC_PER_DAY=$((TOTAL_LOC / DAYS_ELAPSED))
echo -e "Lines of Code Per Day:          ${YELLOW}${LOC_PER_DAY}${NC}"
echo ""

# 4. SERVER PROTOCOLS
echo -e "${GREEN}🖥️  SERVER PROTOCOLS${NC}"
echo "─────────────────────────────────────────────────────────"

TEMP_SERVERS=$(mktemp)

if [ -d "src/server" ]; then
    for protocol_dir in src/server/*/; do
        protocol_name=$(basename "$protocol_dir")
        # Skip non-protocol files
        if [ ! -d "$protocol_dir" ] || [ "$protocol_name" = "mod.rs" ] || [ "$protocol_name" = "connection.rs" ]; then
            continue
        fi

        loc=$(find "$protocol_dir" -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
        if [ "$loc" -gt 0 ]; then
            echo "$loc $protocol_name" >> "$TEMP_SERVERS"
        fi
    done
fi

# Count shared server code
SHARED_LOC=$(find src/server -maxdepth 1 -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
if [ "$SHARED_LOC" -gt 0 ]; then
    echo "$SHARED_LOC [shared code]" >> "$TEMP_SERVERS"
fi

# Sort and display
if [ -f "$TEMP_SERVERS" ] && [ -s "$TEMP_SERVERS" ]; then
    sort -rn "$TEMP_SERVERS" | while read loc name; do
        printf "  %-25s %6s lines\n" "$name" "$loc"
    done
    echo ""
fi

rm -f "$TEMP_SERVERS"

# 5. CLIENT PROTOCOLS
echo -e "${GREEN}📱 CLIENT PROTOCOLS${NC}"
echo "─────────────────────────────────────────────────────────"

TEMP_CLIENTS=$(mktemp)

if [ -d "src/client" ]; then
    for protocol_dir in src/client/*/; do
        protocol_name=$(basename "$protocol_dir")
        loc=$(find "$protocol_dir" -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
        if [ "$loc" -gt 0 ]; then
            echo "$loc $protocol_name" >> "$TEMP_CLIENTS"
        fi
    done
fi

# Count shared client code
SHARED_CLIENT_LOC=$(find src/client -maxdepth 1 -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
if [ "$SHARED_CLIENT_LOC" -gt 0 ]; then
    echo "$SHARED_CLIENT_LOC [shared code]" >> "$TEMP_CLIENTS"
fi

if [ -f "$TEMP_CLIENTS" ] && [ -s "$TEMP_CLIENTS" ]; then
    sort -rn "$TEMP_CLIENTS" | while read loc name; do
        printf "  %-25s %6s lines\n" "$name" "$loc"
    done
    echo ""
else
    echo "  (No client protocols yet)"
    echo ""
fi

rm -f "$TEMP_CLIENTS"

# 6. OTHER MODULES
echo -e "${GREEN}📦 OTHER MODULES${NC}"
echo "─────────────────────────────────────────────────────────"

for module in cli llm protocol state events; do
    if [ -d "src/$module" ]; then
        loc=$(find "src/$module" -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
        if [ "$loc" -gt 0 ]; then
            printf "  %-25s %6s lines\n" "$module" "$loc"
        fi
    fi
done
echo ""

# 7. TEST BREAKDOWN
echo -e "${GREEN}🧪 TEST STATISTICS${NC}"
echo "─────────────────────────────────────────────────────────"

TEST_LOC=$(find tests -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
SRC_LOC=$(find src -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")

echo -e "  Source Code (src/):             ${YELLOW}${SRC_LOC}${NC} lines"
echo -e "  Test Code (tests/):             ${YELLOW}${TEST_LOC}${NC} lines"

if [ "$SRC_LOC" -gt 0 ]; then
    TEST_RATIO=$((TEST_LOC * 100 / SRC_LOC))
    echo -e "  Test-to-Source Ratio:           ${YELLOW}${TEST_RATIO}%${NC}"
fi
echo ""


# 9. CODE DISTRIBUTION PIE
echo -e "${GREEN}🥧 CODE DISTRIBUTION${NC}"
echo "─────────────────────────────────────────────────────────"

TEMP_DISTRIBUTION=$(mktemp)

# Get all major directories
for dir in src/server src/client tests src/llm src/cli src/state src/protocol src/events; do
    if [ -d "$dir" ]; then
        loc=$(find "$dir" -type f -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' || echo "0")
        if [ "$loc" -gt 0 ]; then
            percent=$((loc * 100 / TOTAL_LOC))
            # Create visual bar
            bar_length=$((percent / 2))
            bar=$(printf '█%.0s' $(seq 1 $bar_length))
            printf "  %-20s %5d%% │${YELLOW}%-25s${NC}│ %6d lines\n" "$dir" "$percent" "$bar" "$loc"
        fi
    fi
done

echo ""

# 10. LINES OF CODE PER DAY
echo -e "${GREEN}📅 LINES OF CODE PER DAY${NC}"
echo "─────────────────────────────────────────────────────────"

TEMP_DAILY_STATS=$(mktemp)

# Get unique dates in reverse order (oldest first)
git log --format="%ai" --reverse | cut -d' ' -f1 | sort -u | while read date; do
    # Count commits on or before this date
    commit_count=$(git log --until="$date 23:59:59" --oneline 2>/dev/null | wc -l)

    # Estimate: average of ~440 LOC per commit
    estimated_loc=$((commit_count * 440))

    echo "$date $estimated_loc" >> "$TEMP_DAILY_STATS"
done

# Display the last 15 days
if [ -f "$TEMP_DAILY_STATS" ] && [ -s "$TEMP_DAILY_STATS" ]; then
    tail -15 "$TEMP_DAILY_STATS" | while read date loc; do
        printf "  %s   %7d lines\n" "$date" "$loc"
    done
else
    echo "  (Unable to compute daily stats)"
fi

rm -f "$TEMP_DAILY_STATS"
echo ""

# 11. SUMMARY
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}📈 PRODUCTIVITY SUMMARY${NC}"
echo "─────────────────────────────────────────────────────────"
echo -e "  Total LOC:                      ${YELLOW}${TOTAL_LOC}${NC}"
echo -e "  Days Active:                    ${YELLOW}${DAYS_ELAPSED}${NC}"
echo -e "  Daily Avg LOC:                  ${YELLOW}${LOC_PER_DAY}${NC} lines/day"
echo -e "  Commits:                        ${YELLOW}${COMMITS}${NC}"
echo -e "  LOC per Commit:                 ${YELLOW}$((TOTAL_LOC / COMMITS))${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}\n"
