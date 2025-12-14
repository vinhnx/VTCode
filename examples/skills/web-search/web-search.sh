#!/bin/bash
# Web Search Skill for VTCode
# Integrates web search functionality using curl and public APIs

set -e

# Configuration
TOOL_NAME="web-search"
DEFAULT_API="https://api.duckduckgo.com/"
MAX_RESULTS=10
TIMEOUT=30

# Logging
log() {
    echo "[$TOOL_NAME] $1" >&2
}

error_exit() {
    echo '{"status": "error", "error": "'$1'", "tool": "'$TOOL_NAME'"}' >&2
    exit 1
}

# Parse arguments
QUERY=""
FORMAT="json"
MAX_RESULTS_ARG=""
SAFE_SEARCH="moderate"
REGION="US"
VERBOSE=false
OUTPUT_FILE=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --query|-q)
            QUERY="$2"
            shift 2
            ;;
        --format|-f)
            FORMAT="$2"
            shift 2
            ;;
        --max-results|-m)
            MAX_RESULTS_ARG="$2"
            shift 2
            ;;
        --safe-search|-s)
            SAFE_SEARCH="$2"
            shift 2
            ;;
        --region|-r)
            REGION="$2"
            shift 2
            ;;
        --output|-o)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --help)
            echo "Usage: $0 --query SEARCH_QUERY [OPTIONS]"
            echo "Options:"
            echo "  --query, -q TEXT        Search query (required)"
            echo "  --format, -f FORMAT     Output format: json, text, summary (default: json)"
            echo "  --max-results, -m NUM   Maximum results (default: 10)"
            echo "  --safe-search, -s LEVEL Safe search: strict, moderate, off (default: moderate)"
            echo "  --region, -r CODE       Region code (default: US)"
            echo "  --output, -o FILE       Output file"
            echo "  --verbose, -v          Verbose output"
            echo "  --help                 Show this help"
            exit 0
            ;;
        *)
            # Handle JSON input or unknown args
            if [[ "$1" == "{"* ]]; then
                # Parse JSON input
                if command -v jq >/dev/null 2>&1; then
                    QUERY=$(echo "$1" | jq -r '.query // empty')
                    FORMAT=$(echo "$1" | jq -r '.format // "json"')
                    MAX_RESULTS_ARG=$(echo "$1" | jq -r '.max_results // empty')
                    SAFE_SEARCH=$(echo "$1" | jq -r '.safe_search // "moderate"')
                    REGION=$(echo "$1" | jq -r '.region // "US"')
                else
                    error_exit "jq is required for JSON input parsing"
                fi
            else
                QUERY="$*"
            fi
            break
            ;;
    esac
done

# Validate required arguments
if [[ -z "$QUERY" ]]; then
    error_exit "Search query is required. Use --help for usage information."
fi

# Set max results
if [[ -n "$MAX_RESULTS_ARG" ]]; then
    MAX_RESULTS="$MAX_RESULTS_ARG"
fi

# Verbose logging
if [[ "$VERBOSE" == "true" ]]; then
    log "Starting web search"
    log "Query: $QUERY"
    log "Format: $FORMAT"
    log "Max results: $MAX_RESULTS"
    log "Safe search: $SAFE_SEARCH"
    log "Region: $REGION"
fi

# Build search URL
SEARCH_URL="${SEARCH_API:-$DEFAULT_API}?q=$(printf '%s' "$QUERY" | sed 's/ /+/g')&format=json&no_html=1&skip_disambig=1"

# Add safe search parameter
case "$SAFE_SEARCH" in
    strict)
        SEARCH_URL="$SEARCH_URL&safe=strict"
        ;;
    moderate)
        SEARCH_URL="$SEARCH_URL&safe=moderate"
        ;;
    off)
        # No safe search parameter
        ;;
esac

# Add region parameter
SEARCH_URL="$SEARCH_URL&kl=$REGION"

if [[ "$VERBOSE" == "true" ]]; then
    log "Search URL: $SEARCH_URL"
fi

# Perform the search
if [[ "$VERBOSE" == "true" ]]; then
    log "Executing search request..."
fi

RESPONSE=$(curl -s -m "$TIMEOUT" "$SEARCH_URL" 2>/dev/null || echo "")

if [[ -z "$RESPONSE" ]]; then
    error_exit "Search request failed or timed out"
fi

# Parse and format results
case "$FORMAT" in
    json)
        # Return raw JSON response
        if command -v jq >/dev/null 2>&1; then
            # Pretty print JSON if jq is available
            echo "$RESPONSE" | jq '.' 2>/dev/null || echo "$RESPONSE"
        else
            echo "$RESPONSE"
        fi
        ;;
    text)
        # Extract text results
        if command -v jq >/dev/null 2>&1; then
            # Extract abstract and related topics
            ABSTRACT=$(echo "$RESPONSE" | jq -r '.Abstract // empty')
            RELATED=$(echo "$RESPONSE" | jq -r '.RelatedTopics[]?.Text // empty' | head -n "$MAX_RESULTS")
            
            if [[ -n "$ABSTRACT" ]]; then
                echo "Abstract: $ABSTRACT"
                echo
            fi
            
            if [[ -n "$RELATED" ]]; then
                echo "Related results:"
                echo "$RELATED"
            fi
            
            if [[ -z "$ABSTRACT" && -z "$RELATED" ]]; then
                echo "No results found for: $QUERY"
            fi
        else
            # Fallback text extraction
            echo "$RESPONSE" | grep -o '"Text":"[^"]*"' | sed 's/"Text":"\([^"]*\)"/\1/' | head -n "$MAX_RESULTS"
        fi
        ;;
    summary)
        # Create a summary
        if command -v jq >/dev/null 2>&1; then
            ABSTRACT=$(echo "$RESPONSE" | jq -r '.Abstract // empty')
            HEADING=$(echo "$RESPONSE" | jq -r '.Heading // empty')
            
            SUMMARY="Search results for: $QUERY"
            if [[ -n "$HEADING" ]]; then
                SUMMARY="$SUMMARY\n\nMain result: $HEADING"
            fi
            if [[ -n "$ABSTRACT" ]]; then
                SUMMARY="$SUMMARY\n\nAbstract: $ABSTRACT"
            fi
            
            RESULT_COUNT=$(echo "$RESPONSE" | jq '.RelatedTopics | length // 0')
            if [[ "$RESULT_COUNT" -gt 0 ]]; then
                SUMMARY="$SUMMARY\n\nFound $RESULT_COUNT related topics."
            fi
            
            echo -e "$SUMMARY"
        else
            echo "Search completed for: $QUERY"
            echo "(Install jq for detailed results)"
        fi
        ;;
    *)
        error_exit "Invalid format: $FORMAT. Use json, text, or summary."
        ;;
esac

if [[ "$VERBOSE" == "true" ]]; then
    log "Search completed successfully"
fi

exit 0