# prompt-lens

CLI tool that visualizes prompt structure, tokenizes in real-time, and suggests optimizations to reduce token count without losing intent.

## Install

```bash
cargo build --release
./target/release/prompt-lens analyze "Hello world"
```

## Commands

```
prompt-lens analyze "your prompt here"   # Main analysis: token count, cost, structure
prompt-lens visualize "your prompt"     # Color-coded layer breakdown
prompt-lens optimize "your prompt"     # Suggest token-saving transformations
prompt-lens diff <p1> <p2>              # Compare two prompts
prompt-lens init                         # Create .prompt-lens.yaml config
```

## Examples

```bash
# Analyze from stdin
echo "Please kindly assist me with this task" | prompt-lens analyze

# Analyze a file
prompt-lens analyze --file system-prompt.txt

# Token count only
prompt-lens analyze "Hello world"

# Visualize layers
prompt-lens visualize "You are a helpful assistant that..."

# Compare two prompts
prompt-lens diff "Do X" "Please do X"

# Show cost estimate
prompt-lens analyze --model gpt-4o "Your prompt here"
```

## Options

- `--model` — Model for cost calculation (claude-3-5-sonnet, gpt-4o, gpt-3.5-turbo)
- `--width` — Max characters per line (default: 80)
- `--json` — Output as JSON (for analyze, optimize, diff)

## License

MIT