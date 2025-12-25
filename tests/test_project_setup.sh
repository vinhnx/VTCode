#!/usr/bin/env bash

# Test script for VT Code project management

echo "Testing VT Code project management..."

# Check if ~/.vtcode/projects directory exists
if [ -d "$HOME/.vtcode/projects" ]; then
    echo "✓ Found ~/.vtcode/projects directory"
else
    echo "✗ ~/.vtcode/projects directory not found"
    exit 1
fi

# Test creating a sample project structure
echo "Creating test project structure..."
mkdir -p "$HOME/.vtcode/projects/test-project/config"
mkdir -p "$HOME/.vtcode/projects/test-project/cache"
mkdir -p "$HOME/.vtcode/projects/test-project/embeddings"
mkdir -p "$HOME/.vtcode/projects/test-project/retrieval"

# Check if all directories were created
if [ -d "$HOME/.vtcode/projects/test-project/config" ]; then
    echo "✓ Config directory created"
else
    echo "✗ Config directory not created"
fi

if [ -d "$HOME/.vtcode/projects/test-project/cache" ]; then
    echo "✓ Cache directory created"
else
    echo "✗ Cache directory not created"
fi

if [ -d "$HOME/.vtcode/projects/test-project/embeddings" ]; then
    echo "✓ Embeddings directory created"
else
    echo "✗ Embeddings directory not created"
fi

if [ -d "$HOME/.vtcode/projects/test-project/retrieval" ]; then
    echo "✓ Retrieval directory created"
else
    echo "✗ Retrieval directory not created"
fi

# Create a simple .project metadata file
cat > "$HOME/.vtcode/projects/test-project/.project" << EOF
{
  "name": "test-project",
  "description": "Test project for VTCode",
  "created_at": $(date +%s),
  "updated_at": $(date +%s),
  "root_path": "/tmp/test-project",
  "tags": ["test", "vtcode"]
}
EOF

if [ -f "$HOME/.vtcode/projects/test-project/.project" ]; then
    echo "✓ Project metadata file created"
else
    echo "✗ Project metadata file not created"
fi

echo "Test completed."