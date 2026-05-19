# prompt-lens — Specification

## 1. Concept & Vision

A CLI tool that demystifies prompt engineering by making token costs visible and actionable. Developers paste a prompt and instantly see its token breakdown, structural layers, and optimization opportunities — like a linter for AI prompts.

**Feel:** Fast, terminal-native, colorized output. Zero config needed. Drop-in for any workflow.

---

## 2. Design Language

**Aesthetic:** Terminal-first, data-dense but readable. Inspired by `bandwhich`, `exa`, `bat`.

**Color palette (ANSI):**
- Prompt layers: cyan for system, green for user, yellow for assistant, magenta for tool
- Token counts: white on dim
- Warnings: red for high-cost sections
- Optimizations: green for savings

**Typography:** Monospace terminal output. Box-drawing characters for structure.

**Motion:** Immediate output, no animations. Progress spinners only for multi-file operations.

---

## 3. Layout & Structure

### CLI Commands

```
prompt-lens analyze <prompt|file>   # Main analysis command
prompt-lens visualize <prompt>      # Show colored layer breakdown
prompt-lens optimize <prompt>        # Suggest and apply optimizations
prompt-lens diff <prompt1> <prompt2> # Compare two prompts
prompt-lens init                     # Create .prompt-lens.yaml config
```

### Output Structure (analyze)
```
══╗ prompt-lens analyze
  ╠══ Input: 847 tokens | $0.0026 (@ claude-3-5-sonnet-2024-05-20)
  ╠══ Context: 198k/200k (98.5%)
  ╠══ Breakdown by layer:
  ║   System:     312 tokens ████████████████████░░░░░░░ 36.8%
  ║   User:       428 tokens █████████████████████████░░░░ 50.5%
  ║   Assistant:  107 tokens ██████████░░░░░░░░░░░░░░░░░░ 12.6%
  ╠══ Structural markers: 3 sections, 2 lists, 1 table
  ╠══ ⚠ Costly sections: lines 12-18 (142 tokens, low info density)
  ╠══ Suggestions (5):
  ║   → Remove redundant instruction at line 14 (23 tokens)
  ║   → Merge two identical system prompts into one
  ║   → Replace "Please kindly" with "Please" (-4 tokens)
  ║   → Compress table at line 45 to CSV format (-38 tokens)
  ║   → Move examples to a file and reference via path (-89 tokens)
  ╚══ Estimated savings: 154 tokens (18.2%) → $0.0005
```

---

## 4. Features & Interactions

### Core Features

1. **Real-time Tokenization**
   - Uses tiktoken/cl100k_base encoding (same as Claude's pricing model)
   - Supports GPT-4, GPT-3.5-turbo, Claude-3-5-Sonnet token counts
   - Reads from stdin, file, or direct string argument

2. **Prompt Structure Visualization**
   - Detects layer types: system prompt, user message, assistant response, tool definitions
   - Renders with ANSI color coding
   - Shows nesting depth and section boundaries

3. **Cost Analysis**
   - Calculates cost per token for configurable models
   - Shows context window usage percentage
   - Alerts when approaching limits (80%, 90%, 95%, 99%)

4. **Optimization Engine**
   - Rule-based suggestions (verbose phrases → concise)
   - Detects duplicate instructions
   - Identifies low-density sections
   - Suggests file-referenced content for long examples
   - Shows before/after token delta per suggestion

5. **Prompt Diff**
   - Side-by-side comparison of two prompts
   - Color-coded additions/deletions
   - Token delta summary

6. **Config File (.prompt-lens.yaml)**
   - Default model for cost calculation
   - Custom optimization rules
   - Theme override (ansi/nocolor/json)

### Edge Cases
- Empty prompt: show "No content to analyze" with exit code 1
- File not found: show path + error, exit code 2
- Very long prompt (>100k tokens): stream progress, warn about context
- Binary/non-text file: refuse with error message

---

## 5. Component Inventory

### TokenCounter
- Input: string, model name
- Output: token count, cost estimate
- States: loading (for first-run cache), ready

### PromptParser
- Input: raw prompt text
- Output: structured Layer[] with type, content, start/end offsets
- Detects: markdown code fences, XML tags, JSON blobs, list markers

### Optimizer
- Input: parsed prompt
- Output: Suggestion[] with location, before/after tokens, rationale
- Rules: verbosity reduction, duplicate detection, format compression

### DiffEngine
- Input: two prompts
- Output: DiffResult with additions/deletions/token delta

### Renderer
- Input: analysis result
- Output: ANSI-formatted terminal output or JSON

---

## 6. Technical Approach

**Language:** Rust
**Encoding:** tiktoken-rs (cl100k_base tokenizer)
**CLI:** clap for argument parsing
**Output:** ratatui for rich terminal tables, ansi-terminal for colors
**Config:** serde_yaml for .prompt-lens.yaml

**Architecture:**
```
src/
  main.rs          # CLI entry, clap setup
  tokenizer.rs    # Tiktoken wrapper
  parser.rs       # Prompt layer detection
  optimizer.rs    # Suggestion engine
  diff.rs         # Prompt diff
  render.rs       # ANSI output
  config.rs       # Config file handling
  models.rs       # Pricing model definitions
```

**Build:** Cargo. Single binary, no runtime deps.
**Tests:** Unit tests for tokenizer, parser, optimizer. Integration tests for CLI.

---

## 7. Acceptance Criteria

- [ ] `prompt-lens analyze "Hello world"` outputs token count < 100ms
- [ ] Correct token count matches OpenAI tiktoken for cl100k_base
- [ ] `prompt-lens analyze --model claude-3-5-sonnet` shows cost estimate
- [ ] Visualize command shows color-coded layers
- [ ] Optimize command outputs at least 3 suggestions for a verbose prompt
- [ ] Diff command shows token delta between two prompts
- [ ] init command creates .prompt-lens.yaml
- [ ] Build succeeds with `cargo build --release`
- [ ] Binary runs on Windows without Rust installed (via .exe)
- [ ] Empty input produces error, not crash