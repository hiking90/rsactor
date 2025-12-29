#!/bin/bash
set -e

REPO="hiking90/rsactor"
BRANCH="main"
BASE_URL="https://raw.githubusercontent.com/$REPO/$BRANCH/skills"

SKILLS=(
  "rsactor-actor"
  "rsactor-guide"
  "rsactor-handler"
)

# Default to global installation
INSTALL_DIR="$HOME/.claude/skills"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --local)
      INSTALL_DIR=".claude/skills"
      shift
      ;;
    --dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [OPTIONS]"
      echo ""
      echo "Install rsactor skills for Claude Code"
      echo ""
      echo "Options:"
      echo "  --local     Install to .claude/skills (current project)"
      echo "  --dir DIR   Install to custom directory"
      echo "  -h, --help  Show this help message"
      echo ""
      echo "Default: Install to ~/.claude/skills (global)"
      exit 0
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

echo "Installing rsactor skills to: $INSTALL_DIR"

for skill in "${SKILLS[@]}"; do
  echo "  Downloading $skill..."
  mkdir -p "$INSTALL_DIR/$skill"
  curl -sSL "$BASE_URL/$skill/SKILL.md" -o "$INSTALL_DIR/$skill/SKILL.md"
done

echo "Done! Installed ${#SKILLS[@]} skills."
echo ""
echo "Restart Claude Code to use the new skills."
