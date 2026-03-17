#!/bin/bash
# Add a new model to VT Code
# Usage: ./scripts/add_model.sh

set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== VT Code Model Addition Script ===${NC}\n"

# Prompt for model details
read -p "Model ID (e.g., gpt-5.4-nano): " MODEL_ID
read -p "Display Name (e.g., GPT-5.4 Nano): " DISPLAY_NAME
read -p "Provider (openai/anthropic/gemini): " PROVIDER
read -p "Enum name (PascalCase, e.g., GPT54Nano): " ENUM_NAME
read -p "Generation (e.g., 5.4): " GENERATION
read -p "Context window (e.g., 100000): " CONTEXT
read -p "Description: " DESCRIPTION
read -p "Supports reasoning? (true/false): " REASONING
read -p "Supports tool calls? (true/false): " TOOL_CALLS
read -p "Input modalities (comma-separated, e.g., text or text,image): " INPUT_MODALITIES

# Derive const name from MODEL_ID
CONST_NAME=$(echo "$MODEL_ID" | tr '[:lower:]' '[:upper:]' | tr '.' '_' | tr '-' '_')

echo -e "\n${YELLOW}Summary:${NC}"
echo "  Model ID: $MODEL_ID"
echo "  Enum: $ENUM_NAME"
echo "  Const: ${PROVIDER^^}_$CONST_NAME"
echo "  Provider: $PROVIDER"
echo "  Generation: $GENERATION"
echo ""

read -p "Proceed? (y/n): " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Cancelled."
    exit 1
fi

# Check if provider is valid
if [[ "$PROVIDER" != "openai" && "$PROVIDER" != "anthropic" && "$PROVIDER" != "gemini" ]]; then
    echo "Error: Invalid provider. Use openai, anthropic, or gemini."
    exit 1
fi

# 1. Add to constants
if [ "$PROVIDER" = "openai" ]; then
    CONST_FILE="vtcode-config/src/constants/models/openai.rs"
    echo -e "\n${BLUE}1. Add to $CONST_FILE${NC}"
    echo "   - Add \"$MODEL_ID\" to SUPPORTED_MODELS array"
    echo "   - Add: pub const ${PROVIDER^^}_$CONST_NAME: &str = \"$MODEL_ID\";"
    echo "   Run manually or use editor to insert after existing models."
fi

# 2. JSON metadata
echo -e "\n${BLUE}2. Add to docs/models.json${NC}"
JSON_TEMPLATE=$(cat <<EOF
"$MODEL_ID": {
  "id": "$MODEL_ID",
  "name": "$DISPLAY_NAME",
  "description": "$DESCRIPTION",
  "reasoning": $REASONING,
  "tool_call": $TOOL_CALLS,
  "modalities": {
    "input": [$(echo "$INPUT_MODALITIES" | sed 's/,/","/g' | sed 's/^/"/' | sed 's/$/"/')]
  },
  "output": ["text"]
},
"context": $CONTEXT
}
EOF
)
echo "$JSON_TEMPLATE"

# 3. Model ID enum
echo -e "\n${BLUE}3. Add to vtcode-config/src/models/model_id.rs${NC}"
echo "   /// $DESCRIPTION"
echo "   $ENUM_NAME,"

# 4-10: File snippets
echo -e "\n${BLUE}4. Update as_str.rs${NC}"
echo "   ModelId::$ENUM_NAME => models::${PROVIDER}::${PROVIDER^^}_$CONST_NAME,"

echo -e "\n${BLUE}5. Update display.rs${NC}"
echo "   ModelId::$ENUM_NAME => \"$DISPLAY_NAME\","

echo -e "\n${BLUE}6. Update description.rs${NC}"
echo "   ModelId::$ENUM_NAME => \"$DESCRIPTION\","

echo -e "\n${BLUE}7. Update parse.rs${NC}"
echo "   s if s == models::${PROVIDER}::${PROVIDER^^}_$CONST_NAME => Ok(ModelId::$ENUM_NAME),"

echo -e "\n${BLUE}8. Update collection.rs${NC}"
echo "   Add ModelId::$ENUM_NAME to all_models() vector"

echo -e "\n${BLUE}9. Update capabilities.rs${NC}"
echo "   Add ModelId::$ENUM_NAME to relevant matches (generation, variants, etc.)"

echo -e "\n${BLUE}10. Update provider.rs${NC}"
echo "   Add ModelId::$ENUM_NAME to ${PROVIDER^^} provider match"

# Verify
echo -e "\n${YELLOW}Next Steps:${NC}"
echo "1. Manually apply all changes above to each file"
echo "2. Run: cargo check --package vtcode-config"
echo "3. Run: cargo check --all-targets"
echo "4. Test: cargo run -- /model --help | grep -i '${DISPLAY_NAME}'"
echo ""
echo "See docs/development/ADDING_MODELS.md for detailed instructions."
