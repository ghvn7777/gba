# GBA Core Architecture Design

## 1. Overview

GBA (Geektime Bootcamp Agent) is an AI-powered development automation tool that uses
Claude Agent SDK to drive a structured software development workflow. It takes feature
requirements as input, generates detailed specifications through interactive planning,
and executes phased development with automated code review and verification.

The system operates through three commands:

- **`gba init`** - Initialize a repository for GBA usage
- **`gba plan <feature-slug>`** - Interactive planning session to produce feature specs
- **`gba run <feature-slug>`** - Automated phase-by-phase execution of the plan

## 2. System Architecture

### 2.1 Layered Architecture

```
┌─────────────────────────────────────────────────────┐
│                     gba-cli                         │
│                                                     │
│  ┌─────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │  clap   │  │   ratatui    │  │   Progress     │  │
│  │ (args)  │  │ (plan TUI)   │  │  (run display) │  │
│  └────┬────┘  └──────┬───────┘  └───────┬────────┘  │
│       │              │                   │           │
└───────┼──────────────┼───────────────────┼───────────┘
        │              │                   │
        ▼              ▼                   ▼
┌─────────────────────────────────────────────────────┐
│                     gba-core                        │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │                  Engine                       │   │
│  │  ┌──────┐  ┌──────────┐  ┌────────────────┐  │   │
│  │  │ init │  │   plan   │  │      run       │  │   │
│  │  └──┬───┘  └────┬─────┘  └───┬────────────┘  │   │
│  │     │           │             │                │   │
│  │     └───────────┴─────────────┘                │   │
│  │                  │                             │   │
│  │          ┌───────┴───────┐                     │   │
│  │          │ AgentRunner   │                     │   │
│  │          │ (SDK wrapper) │                     │   │
│  │          └───────┬───────┘                     │   │
│  └──────────────────┼────────────────────────────┘   │
│                     │                                │
│  ┌──────────┐  ┌────┴─────┐  ┌───────────────────┐  │
│  │ Session  │  │  GitOps  │  │  HookRunner       │  │
│  │ (config) │  │(worktree)│  │  (precommit)      │  │
│  └──────────┘  └──────────┘  └───────────────────┘  │
└─────────────────────┬───────────────────────────────┘
                      │
┌─────────────────────┼───────────────────────────────┐
│                  gba-pm                              │
│                     │                                │
│  ┌──────────────────┴───────────────────────────┐   │
│  │            PromptManager                      │   │
│  │  ┌─────────────┐  ┌───────────────────────┐   │   │
│  │  │  minijinja   │  │  Built-in Templates   │   │   │
│  │  │  (renderer)  │  │  + Custom Overrides   │   │   │
│  │  └─────────────┘  └───────────────────────┘   │   │
│  └───────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────┘
                      │
        ┌─────────────┴─────────────┐
        ▼                           ▼
┌──────────────────┐     ┌──────────────────┐
│claude-agent-sdk  │     │      tokio       │
│    -rs (Tyr)     │     │   (async rt)     │
└──────────────────┘     └──────────────────┘
```

### 2.2 Crate Dependency Graph

```
gba-cli
  ├── gba-core
  │     └── gba-pm
  ├── gba-pm  (for custom template overrides in CLI)
  ├── clap
  ├── ratatui / crossterm
  └── tokio

gba-core
  ├── gba-pm
  ├── claude-agent-sdk-rs
  └── tokio

gba-pm
  └── minijinja
```

## 3. Core Workflows

### 3.1 `gba init` - Repository Initialization

```
$ gba init

  ┌─────────┐
  │  Start  │
  └────┬────┘
       │
       ▼
  ┌──────────────────┐    yes    ┌────────────────┐
  │ .gba/ exists?    ├─────────► │  Exit (already │
  │                  │           │  initialized)  │
  └────────┬─────────┘           └────────────────┘
           │ no
           ▼
  ┌──────────────────┐
  │ Create .gba/     │
  │ Create .trees/   │
  │ Update .gitignore│
  └────────┬─────────┘
           │
           ▼
  ┌──────────────────┐
  │ Analyze repo     │ ◄── Agent scans directory structure,
  │ structure        │     languages, frameworks, patterns
  └────────┬─────────┘
           │
           ▼
  ┌──────────────────┐
  │ Generate context │ ◄── Agent creates a single .gba.md
  │ document         │     at the project root
  └────────┬─────────┘
           │
           ▼
  ┌──────────────────┐
  │ Update CLAUDE.md │ ◄── Add references to generated
  │ with references  │     context documents
  └────────┬─────────┘
           │
           ▼
  ┌──────────┐
  │   Done   │
  └──────────┘
```

**Artifacts produced:**
- `.gba/` directory structure
- `.trees/` directory (gitignored)
- `.gba.md` at the project root (codebase context summary)
- Updated `CLAUDE.md` with a reference to `.gba.md`

### 3.2 `gba plan <feature-slug>` - Interactive Planning

```
$ gba plan <feature-slug>

  ┌─────────────┐
  │ Enter TUI   │
  │ (ratatui)   │
  └──────┬──────┘
         │
         ▼
  ┌──────────────────────────────────────────────┐
  │           Interactive Conversation            │
  │                                              │
  │  Asst: What are the feature details?         │
  │  User: I want to build a web frontend...     │
  │  Asst: Here's my proposed approach...        │
  │  User: Needs modification...                 │
  │  Asst: Updated approach. Generate spec?      │
  │  User: Agreed                                │
  │                                              │
  └──────────────────┬───────────────────────────┘
                     │
                     ▼
  ┌──────────────────────────────────────────────┐
  │  Generate Artifacts                          │
  │                                              │
  │  1. Create git worktree in .trees/<slug>     │
  │     (branch out from main)                   │
  │  2. Generate feature specs:                  │
  │     - design.md (architecture, interfaces,   │
  │       data structures)                       │
  │     - verification.md (test criteria,        │
  │       acceptance tests)                      │
  │     - phases.yaml (phased dev plan)          │
  │                                              │
  └──────────────────┬───────────────────────────┘
                     │
                     ▼
  ┌──────────────────────────────────────────────┐
  │  User reviews generated specs                │
  │  Agent: "Plan finished. Call gba run."       │
  └──────────────────────────────────────────────┘
```

**Feature Spec structure (produced by plan, updated by run):**

`phases.yaml` serves as both the plan and the execution record. The `plan` command
generates the spec fields; the `run` command fills in the `result` fields as it executes.

```yaml
# .gba/features/<slug>/phases.yaml

feature: "feature description"

# ── Plan (generated by `gba plan`) ──────────────────────────
phases:
  - name: "Phase 1: Core data structures"
    description: "Implement foundational types and traits"
    tasks:
      - "Define struct X with fields..."
      - "Implement trait Y for..."
    # ── Result (filled by `gba run`) ────────────────────────
    result:
      status: completed          # pending | inProgress | completed | failed
      turns: 12                  # agent API round-trips consumed
      commit: "a1b2c3d"         # commit hash after phase completed
  - name: "Phase 2: Business logic"
    description: "Implement core algorithms"
    tasks:
      - "Implement function Z..."
    result:
      status: completed
      turns: 8
      commit: "e4f5g6h"

verification:
  criteria:
    - "All tests pass"
    - "No clippy warnings"
  testCommands:
    - "cargo test"
    - "cargo clippy -- -D warnings"

# ── Execution Summary (filled by `gba run`) ─────────────────
execution:
  status: completed              # pending | inProgress | completed | failed
  totalTurns: 34                 # sum of all phase + review + verify turns
  review:
    turns: 8
    issuesFound: 2
    issuesFixed: 2
  verification:
    turns: 6
    passed: true
  pr: "https://github.com/org/repo/pull/42"
```

### 3.3 `gba run <feature-slug>` - Phased Execution

Supports **resume**: if a previous run was interrupted, `gba run` reads
`phases.yaml`, skips phases with `result.status: completed`, and continues
from the first incomplete phase. The agent receives context about what
was already completed via the `code/resume.md.j2` prompt template.

```
$ gba run <feature-slug>

  ┌────────────────────┐
  │ Load phases.yaml   │
  │ from .gba/features │
  └─────────┬──────────┘
            │
            ▼
  ┌────────────────────┐     ┌───────────────────────────┐
  │ Any phase with     │ yes │ Resume mode:              │
  │ result.status:     ├────►│ Skip completed phases.    │
  │ completed?         │     │ Use code/resume.md.j2     │
  └─────────┬──────────┘     │ for context.              │
            │ no (fresh)     └──────────┬────────────────┘
            │                           │
            ▼                           │
  ┌────────────────────┐                │
  │ Setup worktree     │◄───────────────┘
  │ (.trees/<slug>)    │
  └─────────┬──────────┘
            │
            ▼
  ┌─────────────────────────────────────────────────┐
  │  FOR EACH INCOMPLETE PHASE                      │
  │  ┌───────────────────────────────────────────┐  │
  │  │                                           │  │
  │  │  ┌──────────────┐                         │  │
  │  │  │ Coding Agent │ ◄── Phase-specific      │  │
  │  │  │ writes code  │     prompt + context     │  │
  │  │  └──────┬───────┘                         │  │
  │  │         │                                 │  │
  │  │         ▼                                 │  │
  │  │  ┌──────────────┐    fail   ┌──────────┐  │  │
  │  │  │  Precommit   ├─────────►│  Agent   │  │  │
  │  │  │  Hooks       │          │  fixes   ├──┤  │
  │  │  │  - build     │ ◄────────┤  issues  │  │  │
  │  │  │  - fmt       │          └──────────┘  │  │
  │  │  │  - clippy    │                         │  │
  │  │  │  - security  │                         │  │
  │  │  └──────┬───────┘                         │  │
  │  │         │ pass                            │  │
  │  │         ▼                                 │  │
  │  │  ┌──────────────┐                         │  │
  │  │  │ Commit phase │                         │  │
  │  │  └──────┬───────┘                         │  │
  │  │         │                                 │  │
  │  └─────────┼─────────────────────────────────┘  │
  │            │                                    │
  └────────────┼────────────────────────────────────┘
               │
               ▼
  ┌─────────────────────────────────────────────────┐
  │  CODE REVIEW                                    │
  │  ┌──────────────┐                               │
  │  │ Review Agent │ ◄── Reviews all phase commits │
  │  └──────┬───────┘                               │
  │         │                                       │
  │         ▼                                       │
  │  ┌──────────────┐  has     ┌──────────────┐     │
  │  │ Valid issues?├────────►│ Coding Agent │     │
  │  │              │         │ fixes issues ├──┐  │
  │  └──────┬───────┘         └──────────────┘  │  │
  │         │ none            ▲                  │  │
  │         │                 └──────────────────┘  │
  └─────────┼───────────────────────────────────────┘
            │
            ▼
  ┌─────────────────────────────────────────────────┐
  │  VERIFICATION                                   │
  │  ┌──────────────────┐                           │
  │  │ Verify Agent     │ ◄── Runs verification     │
  │  │ (test criteria)  │     plan from spec        │
  │  └──────┬───────────┘                           │
  │         │                                       │
  │         ▼                                       │
  │  ┌──────────────┐  fail   ┌──────────────┐     │
  │  │  Pass?       ├────────►│ Coding Agent │     │
  │  │              │         │ fixes issues ├──┐  │
  │  └──────┬───────┘         └──────────────┘  │  │
  │         │ pass            ▲                  │  │
  │         │                 └──────────────────┘  │
  └─────────┼───────────────────────────────────────┘
            │
            ▼
  ┌────────────────────┐
  │   Create PR        │
  └────────────────────┘
```

**TUI progress display during run:**

```
$ gba run 0001_web_frontend

  Running feature: 0001_web_frontend
  [x] Setup worktree
  [x] Phase 1: Core data structures
  [x] Commit phase 1
  [~] Phase 2: Business logic        <-- in progress
  [ ] Commit phase 2
  [ ] Code review
  [ ] Handle review issues
  [ ] Verification
  [ ] Create PR
```

## 4. Crate Design

### 4.1 gba-core

**Responsibility:** Core execution engine. Orchestrates agent sessions for init, plan, and
run workflows. Wraps Claude Agent SDK and manages git operations. Provides a minimal,
stream-based API for the CLI layer.

**Public Interface:**

```rust
// ── Configuration ──────────────────────────────────────────

/// Engine configuration (from CLI flags).
/// Merged with ProjectConfig from .gba/config.yaml at runtime.
/// CLI flags take precedence over config.yaml values.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
pub struct EngineConfig {
    /// Path to the target repository.
    repo_path: PathBuf,

    /// Override Claude model (takes precedence over config.yaml).
    #[builder(default, setter(strip_option))]
    model: Option<String>,

    /// Override max tokens (takes precedence over config.yaml).
    #[builder(default, setter(strip_option))]
    max_tokens: Option<u32>,
}

// ── Engine (main entry point) ──────────────────────────────

/// Core execution engine. All operations are async and return
/// stream-based handles for the CLI to consume.
pub struct Engine { /* private fields */ }

impl Engine {
    /// Create a new engine with the given configuration.
    pub async fn new(config: EngineConfig) -> Result<Self, CoreError>;

    /// Initialize the repository for GBA.
    /// Creates .gba/, .trees/, analyzes repo, generates context docs.
    pub async fn init(&self) -> Result<(), CoreError>;

    /// Start an interactive planning session for a feature.
    /// Returns a PlanSession handle for bidirectional communication.
    pub async fn plan(&self, slug: &str) -> Result<PlanSession, CoreError>;

    /// Execute a feature's development plan phase by phase.
    /// Returns a RunStream handle for consuming progress events.
    pub async fn run(&self, slug: &str) -> Result<RunStream, CoreError>;
}

// ── Plan Session (interactive conversation) ────────────────

/// Handle for an interactive planning session.
/// The CLI drives the conversation by calling next() and respond().
pub struct PlanSession { /* private fields */ }

impl PlanSession {
    /// Get the next event from the planning agent.
    /// Returns None when the session is complete.
    pub async fn next(&mut self) -> Option<PlanEvent>;

    /// Send user input to the planning agent.
    pub async fn respond(&mut self, input: &str) -> Result<(), CoreError>;
}

/// Events emitted during a planning session.
pub enum PlanEvent {
    /// Agent produced a text message to display.
    Message(String),

    /// Agent is waiting for user input.
    WaitingForInput,

    /// Agent generated a spec file.
    SpecGenerated { path: PathBuf, content: String },

    /// Planning session completed successfully.
    Completed,

    /// An error occurred.
    Error(CoreError),
}

// ── Run Stream (progress events) ───────────────────────────

/// Handle for consuming run execution progress.
pub struct RunStream { /* private fields */ }

impl RunStream {
    /// Get the next event from the run execution.
    /// Returns None when execution is complete.
    pub async fn next(&mut self) -> Option<RunEvent>;
}

/// Events emitted during feature execution.
pub enum RunEvent {
    /// Execution started.
    Started { feature: String, total_phases: usize },

    /// A development phase started.
    PhaseStarted { index: usize, name: String },

    /// Coding agent is producing output.
    CodingOutput(String),

    /// Precommit hook result.
    HookResult { hook: String, passed: bool },

    /// A phase was committed.
    PhaseCommitted { index: usize, commit_hash: String },

    /// Code review started.
    ReviewStarted,

    /// Code review completed.
    ReviewCompleted { issues: Vec<Issue> },

    /// Verification started.
    VerificationStarted,

    /// Verification completed.
    VerificationCompleted { passed: bool, details: String },

    /// Pull request created.
    PrCreated { url: String },

    /// Execution finished.
    Finished,

    /// An error occurred.
    Error(CoreError),
}

/// A code review issue found by the review agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub file: PathBuf,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Suggestion,
}

// ── Errors ─────────────────────────────────────────────────

/// Core engine errors.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not initialized: run `gba init` first")]
    NotInitialized,

    #[error("already initialized")]
    AlreadyInitialized,

    #[error("feature not found: {0}")]
    FeatureNotFound(String),

    #[error("feature spec missing or invalid: {0}")]
    InvalidSpec(String),

    #[error("agent error: {0}")]
    Agent(String),

    #[error("git operation failed: {0}")]
    Git(String),

    #[error("prompt error")]
    Prompt(#[from] gba_pm::PmError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

**Internal modules (private):**

| Module        | Responsibility                                              |
|---------------|-------------------------------------------------------------|
| `agent`       | Wraps `claude-agent-sdk-rs`, manages agent sessions         |
| `git`         | Git worktree creation, branch management, commit, PR        |
| `hooks`       | Runs precommit hooks (build, fmt, clippy, security checks)  |
| `spec`        | Parses and validates feature specs (phases.yaml, design.md) |
| `init`        | Init workflow implementation                                |
| `plan`        | Plan workflow implementation                                |
| `run`         | Run workflow implementation with phase loop                 |

### 4.2 gba-pm

**Responsibility:** Prompt template management. Loads built-in and custom Jinja2 templates,
renders them with context variables. Provides a simple load-and-render API.

**Public Interface:**

```rust
// ── Prompt Manager ─────────────────────────────────────────

/// Manages prompt templates. Supports built-in templates (compiled
/// into the binary) and custom overrides loaded from disk.
pub struct PromptManager { /* private: minijinja::Environment */ }

impl PromptManager {
    /// Create a manager pre-loaded with built-in templates.
    pub fn new() -> Result<Self, PmError>;

    /// Load custom templates from a directory. Templates in this
    /// directory override built-in templates with the same name.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), PmError>;

    /// Render a named template with the given context.
    pub fn render(&self, name: &str, ctx: &serde_json::Value) -> Result<String, PmError>;

    /// List all available template names.
    pub fn list_templates(&self) -> Vec<&str>;
}

// ── Errors ─────────────────────────────────────────────────

/// Prompt manager errors.
#[derive(Debug, thiserror::Error)]
pub enum PmError {
    #[error("template not found: {0}")]
    TemplateNotFound(String),

    #[error("render error: {0}")]
    RenderError(String),

    #[error("invalid template: {0}")]
    InvalidTemplate(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

**Agent Definitions:**

Each agent is defined in the `agents/` directory at the workspace root. An agent
directory contains a `config.yml` (session configuration) and Jinja2 prompt templates.
`gba-pm` embeds these at compile time; users can override by placing files with the
same relative path in `.gba/agents/`.

Each `system.md.j2` is the **system prompt** (agent role + rules). All other `.md.j2`
files are **user prompts** (task instructions sent as user messages).

```
agents/
├── init/
│   ├── config.yml              # preset: true
│   ├── system.md.j2            # System: repo analyzer
│   └── task.md.j2              # User: analyze repo, generate context
├── plan/
│   ├── config.yml              # preset: true
│   ├── system.md.j2            # System: architect / planner
│   └── task.md.j2              # User: start planning conversation
├── code/
│   ├── config.yml              # preset: true
│   ├── system.md.j2            # System: developer
│   ├── task.md.j2              # User: implement a phase (fresh run)
│   ├── resume.md.j2            # User: implement a phase (resumed run)
│   ├── hook_fix.md.j2          # User: fix failing precommit hooks
│   └── pr.md.j2                # User: create PR via gh CLI
├── review/
│   ├── config.yml              # preset: false (no tools, text analysis only)
│   ├── system.md.j2            # System: code reviewer
│   ├── task.md.j2              # User: review all changes
│   └── fix.md.j2               # User: fix specific review issues
└── verify/
    ├── config.yml              # preset: true
    ├── system.md.j2            # System: QA / tester
    ├── task.md.j2              # User: run verification plan
    └── fix.md.j2               # User: fix verification failures
```

**`config.yml` format:**

```yaml
# preset: true  → SystemPrompt::Preset("claude_code", append=rendered system.md.j2)
#                  Agent gets Claude Code's built-in tools.
# preset: false → SystemPrompt::Text(rendered system.md.j2)
#                  Agent has no tools, pure text analysis.
preset: true

# Restrict to specific tools. [] = all tools available (default).
# Example: ["Read", "Grep", "Glob"] for read-only agents.
tools: []

# Disallow specific tools. [] = nothing disallowed (default).
# Example: ["Write", "Edit", "Bash"] to prevent modifications.
disallowedTools: []
```

**Preset mapping and rationale:**

| Agent  | Preset | Reason                                                  |
|--------|--------|---------------------------------------------------------|
| init   | true   | Needs file read/write and bash to explore & scaffold    |
| plan   | true   | Needs file read/write to generate spec files            |
| code   | true   | Full development: edit, build, test, git                |
| review | false  | Pure analysis — diff provided in prompt, no tool needed |
| verify | true   | Needs bash to run test commands                         |

**Engine usage:**

```rust
// Engine reads config.yml and renders system.md.j2, then:
let system_prompt = if agent_config.preset {
    SystemPrompt::Preset(SystemPromptPreset::with_append(
        "claude_code",
        rendered_system,
    ))
} else {
    SystemPrompt::Text(rendered_system)
};

let mut opts = ClaudeAgentOptions::builder()
    .system_prompt(system_prompt)
    .disallowed_tools(agent_config.disallowed_tools);

// tools: [] means all tools; non-empty restricts to listed tools only
if !agent_config.tools.is_empty() {
    opts = opts.tools(agent_config.tools);
}

let options = opts.build();
```

**Template context variables:**

| Template              | Key Context Variables                                      | Source                          |
|-----------------------|------------------------------------------------------------|---------------------------------|
| `init/system`         | `repo_path`                                                | EngineConfig                    |
| `init/task`           | `repo_tree`                                                | Engine lists directories        |
| `plan/system`         | `repo_path`, `feature_slug`                                | EngineConfig, CLI arg           |
| `plan/task`           | `feature_slug`                                             | CLI arg (agent reads `.gba.md`) |
| `code/system`         | `repo_path`, `feature_slug`, `design_spec`                 | EngineConfig, reads design.md   |
| `code/task`           | `phase`, `phase_index`, `total_phases`                     | phases.yaml + execution state   |
| `code/resume`         | `phase`, `phase_index`, `total_phases`, `completed_phases` | phases.yaml + execution state   |
| `code/hook_fix`       | `hook_name`, `hook_command`, `hook_output`                 | HooksConfig + captured output   |
| `code/pr`             | `feature_slug`, `branch`, `base_branch`, `feature_description`, `phases`, `review`, `verification` | config.yaml + phases.yaml |
| `review/system`       | `repo_path`, `feature_slug`                                | EngineConfig, CLI arg           |
| `review/task`         | `feature_slug`, `design_spec`, `verification_criteria`, `diff` | reads design.md, phases.yaml, `git diff` |
| `review/fix`          | `issues`                                                   | Parsed from review agent output |
| `verify/system`       | `repo_path`, `feature_slug`                                | EngineConfig, CLI arg           |
| `verify/task`         | `feature_slug`, `design_spec`, `criteria`, `test_commands` | reads design.md, phases.yaml    |
| `verify/fix`          | `failures`, `output`                                       | Parsed from verify agent output |

### 4.3 gba-cli

**Responsibility:** User-facing CLI and TUI. Parses commands, drives gba-core operations,
and renders interactive UI for planning and progress display.

**Commands:**

```
gba init                   Initialize current repo for GBA
gba plan <feature-slug>    Start interactive planning session
gba run  <feature-slug>    Execute feature plan phase by phase
```

**Internal Modules:**

| Module     | Responsibility                                           |
|------------|----------------------------------------------------------|
| `cli`      | Clap command definitions and argument parsing            |
| `tui`      | Ratatui application shell (event loop, layout, keybinds) |
| `plan_ui`  | Plan session UI: chat-style conversation view            |
| `run_ui`   | Run progress UI: checkbox progress list                  |

**CLI ↔ Core interaction pattern:**

```rust
// Plan command: bidirectional stream
async fn handle_plan(engine: &Engine, slug: &str) -> Result<()> {
    let mut session = engine.plan(slug).await?;
    let mut app = PlanApp::new();

    loop {
        match session.next().await {
            Some(PlanEvent::Message(text)) => app.show_message(text),
            Some(PlanEvent::WaitingForInput) => {
                let input = app.get_user_input().await;
                session.respond(&input).await?;
            }
            Some(PlanEvent::Completed) => break,
            None => break,
            // ...
        }
    }
    Ok(())
}

// Run command: unidirectional stream
async fn handle_run(engine: &Engine, slug: &str) -> Result<()> {
    let mut stream = engine.run(slug).await?;
    let mut app = RunApp::new();

    while let Some(event) = stream.next().await {
        app.handle_event(event);
        app.render()?;
    }
    Ok(())
}
```

## 5. Data Model

### 5.1 Feature Spec

```rust
/// Feature specification and execution record.
/// Serialized as phases.yaml. Plan fields are written by `gba plan`,
/// result fields are written by `gba run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureSpec {
    /// Human-readable feature description.
    pub feature: String,

    /// Ordered development phases.
    pub phases: Vec<Phase>,

    /// Verification criteria and commands.
    pub verification: VerificationPlan,

    /// Execution summary, filled by `gba run`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<Execution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    pub name: String,
    pub description: String,
    pub tasks: Vec<String>,

    /// Execution result for this phase, filled by `gba run`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<PhaseResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseResult {
    pub status: StepStatus,
    /// Number of agent API round-trips consumed.
    pub turns: u32,
    /// Commit hash after phase completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationPlan {
    pub criteria: Vec<String>,
    pub test_commands: Vec<String>,
}

/// Overall execution summary, written to phases.yaml by `gba run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub status: StepStatus,
    /// Total agent turns across all phases + review + verification.
    pub total_turns: u32,
    pub review: ReviewResult,
    pub verification: VerificationResult,
    /// PR URL, set after PR is created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub turns: u32,
    pub issues_found: u32,
    pub issues_fixed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub turns: u32,
    pub passed: bool,
}
```

`phases.yaml` is the single source of truth for both plan and execution state.
When `gba run` is interrupted and resumed, it reads `phases.yaml` to determine
which phases are already completed (by checking `result.status`) and continues
from the first non-completed phase.

## 6. Project Directory Convention

### 6.1 `.gba/` Directory (checked into git)

```
.gba/
├── config.yaml                     # GBA project configuration
└── features/
    ├── 0001_feature_slug/
    │   ├── specs/
    │   │   ├── design.md           # Architecture & interface design
    │   │   └── verification.md     # Test criteria & acceptance tests
    │   ├── docs/
    │   │   └── impl_details.md     # Implementation notes (generated during run)
    │   └── phases.yaml             # Phased development plan
    └── 0002_another_feature/
        └── ...
```

### 6.2 `.gba/config.yaml`

```yaml
# GBA project configuration
# Generated by `gba init`, editable by user.

# ── Agent Configuration ─────────────────────────────────────
agent:
  # Claude model to use (optional, SDK handles default).
  # model: claude-sonnet-4-20250514

  # Max tokens per agent response.
  # maxTokens: 16384

  # Permission mode for agent tool use:
  #   auto   - agent runs tools without asking (default)
  #   manual - agent asks before each tool invocation
  #   none   - agent cannot use tools (prompt-only mode)
  permissionMode: auto

# ── Prompt Configuration ────────────────────────────────────
prompts:
  # Additional prompt template directories (optional).
  # Templates here override built-in defaults by name.
  # Searched in order; first match wins.
  include: []
    # - ~/.config/gba/prompts
    # - ./custom-prompts

# ── Git Configuration ───────────────────────────────────────
git:
  # Automatically commit after each phase completes.
  autoCommit: true

  # Branch naming pattern for feature worktrees.
  # Variables: {id} = feature number, {slug} = feature slug
  branchPattern: "feat/{id}-{slug}"

  # Base branch to create worktrees from.
  baseBranch: main

# ── Code Review Configuration ───────────────────────────────
review:
  # Enable code review step after all phases complete.
  enabled: true

  # Maximum review-fix iterations before proceeding.
  maxIterations: 3

# ── Verification Configuration ──────────────────────────────
verification:
  # Enable verification step after code review.
  enabled: true

  # Maximum verify-fix iterations before failing.
  maxIterations: 3

# ── Hooks Configuration ─────────────────────────────────────
# Hooks run after each phase's code is written, before commit.
# Each hook is a shell command executed in the worktree root.
# If any hook fails, the agent attempts to fix and retry.
hooks:
  preCommit:
    - name: build
      command: cargo build
    - name: fmt
      command: cargo +nightly fmt --check
    - name: lint
      command: cargo clippy -- -D warnings
    # - name: security
    #   command: cargo audit

  # Maximum hook-fix-retry cycles per phase.
  maxRetries: 5
```

**Corresponding Rust type (in `gba-core`):**

```rust
/// Project-level GBA configuration, deserialized from .gba/config.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub prompts: PromptsConfig,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub review: ReviewConfig,
    #[serde(default)]
    pub verification: VerificationConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: PermissionMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Auto,
    Manual,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptsConfig {
    #[serde(default)]
    pub include: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfig {
    #[serde(default = "default_true")]
    pub auto_commit: bool,
    #[serde(default = "default_branch_pattern")]
    pub branch_pattern: String,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksConfig {
    #[serde(default = "default_pre_commit_hooks")]
    pub pre_commit: Vec<Hook>,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    pub name: String,
    pub command: String,
}
```

**Config loading flow:**

```
Engine::new(EngineConfig)
    │
    ├── EngineConfig.repo_path   ◄── CLI flag or cwd
    │
    ├── Load .gba/config.yaml    ◄── ProjectConfig (per-project)
    │   └── Merge with EngineConfig (CLI flags override config.yaml)
    │
    └── Final merged config used by all operations
```

CLI flags take precedence over `config.yaml` values. For example,
`gba run --model claude-opus-4 0001_feature` overrides `agent.model` from the config file.

### 6.3 `.trees/` Directory (gitignored)

```
.trees/
├── 0001_feature_slug/              # git worktree (branch: feat/0001_feature_slug)
└── 0002_another_feature/           # git worktree (branch: feat/0002_another_feature)
```

### 6.4 Generated Context (from `gba init`)

```
project-root/
├── .gba.md                         # Codebase context summary (single file)
├── CLAUDE.md                       # Updated with reference to .gba.md
└── ...
```

`.gba.md` contains a single, comprehensive summary of the repository:
project structure, key modules, languages, frameworks, conventions,
and important types/interfaces. This file is referenced from `CLAUDE.md`
so that AI coding agents pick it up automatically.

## 7. Development Plan

### Phase 1: Prompt Manager (`gba-pm`)

- Implement `PromptManager::new()` with embedded built-in templates
- Implement `PromptManager::load_dir()` for custom template overrides
- Implement `PromptManager::render()` with minijinja
- Implement `PromptManager::list_templates()`
- Create initial prompt templates (init, plan, code, review, verify)
- Unit tests for template loading and rendering

### Phase 2: Core Foundation (`gba-core`)

- Define `EngineConfig`, `CoreError`, and all public data types
- Implement `Engine::new()` with config validation
- Implement internal `AgentRunner` wrapping `claude-agent-sdk-rs`
- Implement internal `GitOps` (worktree creation, branch, commit)
- Implement internal `HookRunner` (precommit hooks executor)
- Implement `FeatureSpec` and `RunState` serialization
- Unit tests for internal modules

### Phase 3: Init Workflow

- Implement `Engine::init()` flow
- Create `init_system` prompt template with repo analysis instructions
- Implement `.gba/` and `.trees/` directory creation
- Implement `.gba.md` context document generation at project root
- Implement `CLAUDE.md` update logic
- Integration test with a sample repository

### Phase 4: Plan Workflow

- Implement `Engine::plan()` returning `PlanSession`
- Implement `PlanSession` with bidirectional agent communication
- Create `plan_system` prompt template
- Implement spec file generation (design.md, verification.md, phases.yaml)
- Implement git worktree creation for feature branches
- Update `gba-cli` with `plan` command and TUI chat interface
- Integration test for plan conversation flow

### Phase 5: Run Workflow

- Implement `Engine::run()` returning `RunStream`
- Implement phase-by-phase execution loop
- Implement coding agent → precommit hooks → fix cycle
- Create `code_system` prompt template
- Implement code review agent loop
- Create `review_system` prompt template
- Implement verification agent loop
- Create `verify_system` prompt template
- Implement PR creation
- Implement `RunState` persistence for resume support
- Update `gba-cli` with `run` command and progress TUI
- Integration test for full run workflow

### Phase 6: Polish & Hardening

- Error recovery and graceful degradation
- Tracing instrumentation across all crates
- Edge case handling (network failures, agent errors, git conflicts)
- Documentation (rustdoc for all public items)
- End-to-end test with a real repository
