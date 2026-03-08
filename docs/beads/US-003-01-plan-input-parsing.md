# US-003-01: Plan input parsing and CLI args

**Parent:** US-003 (`rewind plan`)
**Size:** S
**Depends on:** —

## Goal
Accept plan input from positional arg, `-f` file, or stdin. Parse into a raw string for downstream processing.

## Tasks
1. Update `Commands::Plan` in `main.rs` with clap args:
   ```rust
   Plan {
       /// Task or PRD description
       description: Option<String>,
       /// Read description from file
       #[arg(short, long)]
       file: Option<PathBuf>,
       /// Print plan without persisting
       #[arg(long)]
       dry_run: bool,
   }
   ```
2. In `commands/plan.rs`, implement input resolution:
   ```rust
   fn resolve_input(description: Option<String>, file: Option<PathBuf>) -> Result<String, String>
   ```
   - If `description` is Some → use it
   - If `file` is Some → read file contents
   - Else → read stdin (with atty check: error if stdin is a TTY with no input)
3. Unit test: resolve_input with each source

## Files touched
- `crates/rewind-cn/src/main.rs` (modify)
- `crates/rewind-cn/src/commands/plan.rs` (rewrite)

## Done when
- `rewind plan "do the thing"` captures input
- `rewind plan -f todo.md` reads file
- `echo "do it" | rewind plan` reads from stdin
- All three paths tested
