//! `brain` — version control for AI agent memory.
//!
//! v0.1 ships `init`, `onboard`, `note`, `log`, `ask`, `serve --mcp`,
//! `doctor`, `tui`, and git sync commands against the in-process `LocalBrain`
//! adapter from brain-app. `dream` is reserved and currently unimplemented.

use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use brain_app::LocalBrain;
use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use regex::Regex;

/// Path resolution precedence:
///   1. `--brain-dir <PATH>` flag
///   2. `BRAIN_DIR` env var
///   3. `~/.brain` (default)
#[derive(Parser, Debug)]
#[command(name = "brain", version, about = "version control for AI agent memory")]
struct Cli {
    /// Path to the brain repo. Overrides BRAIN_DIR and ~/.brain.
    #[arg(long, global = true, env = "BRAIN_DIR")]
    brain_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Guided first-run setup.
    Onboard {
        /// Agent harnesses to wire. Use comma-separated ids, `all`, or `none`.
        #[arg(long, value_delimiter = ',')]
        agents: Vec<String>,
        /// Apply defaults without prompting. Does not wire agents unless --agents is set.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Refresh managed prompt blocks/files that were written by brain before.
        #[arg(long)]
        reconfigure: bool,
        /// Project directory for project-local adapters like Cursor and Hermes.
        #[arg(long)]
        project_dir: Option<PathBuf>,
    },
    /// Set up a new brain.
    Init,
    /// Save a note.
    Note {
        /// What to remember.
        text: String,
    },
    /// Show recent notes.
    Log,
    /// Search your notes.
    Ask { query: String },
    /// Start the MCP server (stdio).
    Serve {
        #[arg(long)]
        mcp: bool,
    },
    /// Open the full-screen TUI.
    Tui,
    /// Show brain status.
    Doctor {
        /// Force a full index rebuild from git before reporting. Useful
        /// when `ask` / `log` look inconsistent or after a history rewrite.
        #[arg(long)]
        deep: bool,
    },
    /// Reserved for future memory compaction (not implemented yet).
    Dream,
    /// Push brain commits to a remote git repo.
    Push {
        /// Remote name (default: `origin`).
        remote: Option<String>,
    },
    /// Pull brain commits from a remote git repo (rebase).
    Pull {
        /// Remote name (default: `origin`).
        remote: Option<String>,
    },
    /// Manage remotes.
    Remote {
        #[command(subcommand)]
        cmd: RemoteCmd,
    },
}

#[derive(Subcommand, Debug)]
enum RemoteCmd {
    /// Add a new remote.
    Add { name: String, url: String },
    /// List configured remotes.
    List,
}

fn main() -> Result<()> {
    let args = Cli::parse();

    // CRITICAL: write logs to stderr, never stdout. The `brain serve --mcp`
    // transport uses stdout for JSON-RPC; any log line on stdout corrupts
    // the protocol stream and breaks every MCP client.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let brain_dir = resolve_brain_dir(args.brain_dir)?;

    match args.command {
        Command::Onboard {
            agents,
            yes,
            reconfigure,
            project_dir,
        } => cmd_onboard(
            &brain_dir,
            OnboardOptions {
                agents,
                yes,
                reconfigure,
                project_dir,
            },
        ),
        Command::Init => cmd_init(&brain_dir),
        Command::Note { text } => cmd_note(&brain_dir, text),
        Command::Log => cmd_log(&brain_dir),
        Command::Ask { query } => cmd_ask(&brain_dir, query),
        Command::Dream => anyhow::bail!("dream: not yet"),
        Command::Serve { mcp } => cmd_serve(&brain_dir, mcp),
        Command::Tui => cmd_tui(&brain_dir),
        Command::Doctor { deep } => cmd_doctor(&brain_dir, deep),
        Command::Push { remote } => cmd_push(&brain_dir, remote.as_deref()),
        Command::Pull { remote } => cmd_pull(&brain_dir, remote.as_deref()),
        Command::Remote { cmd } => cmd_remote(&brain_dir, cmd),
    }
}

struct Palette {
    enabled: bool,
}

impl Palette {
    fn detect() -> Self {
        Self {
            enabled: std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
                && std::env::var_os("TERM").is_none_or(|term| term != "dumb"),
        }
    }

    fn paint(&self, code: &str, text: impl AsRef<str>) -> String {
        if self.enabled {
            format!("\x1b[{code}m{}\x1b[0m", text.as_ref())
        } else {
            text.as_ref().to_string()
        }
    }

    fn title(&self, text: impl AsRef<str>) -> String {
        self.paint("1;36", text)
    }

    fn step(&self, n: usize, total: usize, text: impl AsRef<str>) -> String {
        format!(
            "{} {}",
            self.paint("1;36", format!("[{n}/{total}]")),
            self.paint("1", text)
        )
    }

    fn ok(&self, text: impl AsRef<str>) -> String {
        self.paint("1;32", text)
    }

    fn warn(&self, text: impl AsRef<str>) -> String {
        self.paint("1;33", text)
    }

    fn dim(&self, text: impl AsRef<str>) -> String {
        self.paint("2", text)
    }

    fn command(&self, text: impl AsRef<str>) -> String {
        self.paint("1;33", text)
    }
}

struct OnboardOptions {
    agents: Vec<String>,
    yes: bool,
    reconfigure: bool,
    project_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentHarness {
    ClaudeCode,
    Cursor,
    Codex,
    OpenClaw,
    Hermes,
}

impl AgentHarness {
    const ALL: [Self; 5] = [
        Self::ClaudeCode,
        Self::Cursor,
        Self::Codex,
        Self::OpenClaw,
        Self::Hermes,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
            Self::OpenClaw => "OpenClaw",
            Self::Hermes => "Hermes",
        }
    }

    fn wiring_summary(self) -> &'static str {
        match self {
            Self::ClaudeCode => "MCP stdio + user-wide CLAUDE.md memory prompt",
            Self::Cursor => "MCP stdio + project .cursor alwaysApply rule",
            Self::Codex => "MCP stdio + user-wide AGENTS.md memory prompt",
            Self::OpenClaw => "workspace BRAIN.md with CLI memory instructions",
            Self::Hermes => "project AGENTS.md with CLI memory instructions",
        }
    }
}

#[derive(Debug)]
struct WiringContext {
    home: PathBuf,
    project_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannedAction {
    Create,
    Append,
    Replace,
    SkipAlreadyConfigured,
    SkipExists,
}

impl PlannedAction {
    fn label(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Append => "append",
            Self::Replace => "replace",
            Self::SkipAlreadyConfigured => "already configured",
            Self::SkipExists => "exists; merge manually",
        }
    }

    fn is_write(self) -> bool {
        matches!(self, Self::Create | Self::Append | Self::Replace)
    }
}

#[derive(Debug)]
enum WiringOpKind {
    WriteFileIfMissing {
        content: &'static str,
    },
    WriteManagedFile {
        content: &'static str,
    },
    AppendManagedBlock {
        content: &'static str,
    },
    AppendIfMissingText {
        marker: &'static str,
        content: &'static str,
    },
}

#[derive(Debug)]
struct WiringOp {
    harness: AgentHarness,
    path: PathBuf,
    description: &'static str,
    kind: WiringOpKind,
}

const CLAUDE_MCP: &str = include_str!("../../../adapters/claude-code/mcp.json");
const CLAUDE_BRAIN: &str = include_str!("../../../adapters/claude-code/brain.md");
const CURSOR_MCP: &str = include_str!("../../../adapters/cursor/mcp.json");
const CURSOR_RULE: &str = include_str!("../../../adapters/cursor/brain.mdc");
const CODEX_CONFIG: &str = include_str!("../../../adapters/codex/config.toml");
const CODEX_AGENTS: &str = include_str!("../../../adapters/codex/AGENTS.md");
const OPENCLAW_BRAIN: &str = include_str!("../../../adapters/openclaw/brain-cli.md");
const HERMES_BRAIN: &str = include_str!("../../../adapters/hermes/brain-cli.md");

fn cmd_onboard(brain_dir: &PathBuf, opts: OnboardOptions) -> Result<()> {
    let p = Palette::detect();
    let project_dir = opts
        .project_dir
        .map(absolute_path)
        .transpose()?
        .unwrap_or(std::env::current_dir().context("current directory")?);
    let wiring_ctx = WiringContext {
        home: resolve_home_dir()?,
        project_dir,
    };

    println!("{}", p.title("brain onboard"));
    println!(
        "{}",
        p.dim("Git-backed long-term memory for AI coding agents.")
    );
    println!();

    println!("{}", p.step(1, 6, "What you'll need"));
    println!("  - A macOS shell with the brain binary on PATH.");
    println!("  - Git available locally.");
    println!("  - Optional: choose which agents can read/write this memory.");
    println!();

    println!("{}", p.step(2, 6, "Local memory"));
    let brain = match LocalBrain::open(brain_dir) {
        Ok(brain) => {
            println!(
                "  {} {}",
                p.ok("kept existing brain at"),
                brain.path().display()
            );
            brain
        }
        Err(e) if e.is_not_found() => {
            let brain = LocalBrain::init(brain_dir)
                .with_context(|| format!("init at {}", brain_dir.display()))?;
            println!("  {} {}", p.ok("created"), brain.path().display());
            brain
        }
        Err(e) => return Err(e).with_context(|| format!("open at {}", brain_dir.display())),
    };
    println!(
        "  {}",
        p.dim("reset is never automatic; back up and remove the directory yourself if needed.")
    );
    println!();

    println!("{}", p.step(3, 6, "What onboarding wrote"));
    println!(
        "  - {} {}",
        p.title("git event log:"),
        brain.path().display()
    );
    println!(
        "  - {} {}/.brain/index.sqlite",
        p.title("derived SQLite index:"),
        brain.path().display()
    );
    println!("  - {}", p.dim("no cloud account, daemon, or credentials."));
    println!();

    println!("{}", p.step(4, 6, "Choose agents"));
    let selected_agents = choose_onboard_agents(&p, &opts.agents, opts.yes)?;
    if selected_agents.is_empty() {
        println!(
            "  {}",
            p.dim("no agent wiring selected; memory is still usable from the CLI.")
        );
        println!(
            "  {}",
            p.dim("later: brain onboard --agents claude-code,codex --yes")
        );
    } else {
        for agent in &selected_agents {
            println!(
                "  - {} {}",
                p.title(agent.label()),
                p.dim(agent.wiring_summary())
            );
        }
    }
    println!();

    println!("{}", p.step(5, 6, "Save wiring"));
    let plan = build_wiring_plan(&selected_agents, &wiring_ctx);
    if plan.is_empty() {
        println!("  {}", p.dim("nothing to write."));
    } else {
        print_wiring_preview(&p, &plan, opts.reconfigure)?;
        if should_apply_wiring(&p, opts.yes)? {
            let results = apply_wiring_plan(&plan, opts.reconfigure)?;
            for (op, action) in plan.iter().zip(results) {
                println!(
                    "  {} {} ({})",
                    if action.is_write() {
                        p.ok(action.label())
                    } else {
                        p.dim(action.label())
                    },
                    op.path.display(),
                    op.harness.label()
                );
            }
        } else {
            println!("  {}", p.dim("wiring not saved."));
        }
    }
    println!();

    println!("{}", p.step(6, 6, "Health check"));
    let report = brain.doctor().context("doctor")?;
    if report.issues.is_empty() {
        println!(
            "  {} {} note{} {}",
            p.ok("ready:"),
            report.event_count,
            if report.event_count == 1 { "" } else { "s" },
            p.dim(format!("({} indexed)", report.indexed_event_count))
        );
        if !report.warnings.is_empty() {
            println!("  {}", p.warn("warnings:"));
            for warning in &report.warnings {
                println!("    - {warning}");
            }
        }
    } else {
        println!("  {}", p.warn("problems:"));
        for issue in &report.issues {
            println!("    - {issue}");
        }
        anyhow::bail!("{} issue(s)", report.issues.len());
    }
    println!();

    println!("{}", p.title("Try it now"));
    println!(
        "  {}",
        p.command("brain note \"remember that auth uses PKCE\"")
    );
    println!("  {}", p.command("brain ask \"auth\""));
    println!("  {}", p.command("brain log"));
    println!("  {}", p.command("brain tui"));
    println!();

    println!("{}", p.title("Script it"));
    println!(
        "  {}",
        p.command("brain onboard --agents claude-code,cursor,codex --yes")
    );
    println!("  {}", p.command("brain onboard --agents all --yes"));
    println!("  {}", p.command("brain onboard --agents none --yes"));
    println!();

    println!("{}", p.title("Next steps"));
    println!("  - Use brain doctor --deep if search/log ever look inconsistent.");
    println!("  - Use brain remote add origin <url>, then brain push / brain pull for sync.");
    println!("  - Re-run brain onboard --reconfigure to refresh managed wiring blocks.");

    Ok(())
}

fn parse_agent_selection(raw: &str) -> Result<Vec<AgentHarness>> {
    static ALL_RE: Lazy<Regex> = Lazy::new(|| word_regex("all"));
    static NONE_RE: Lazy<Regex> = Lazy::new(|| word_regex("none|no"));
    static AGENT_RE: Lazy<Vec<(AgentHarness, Regex)>> = Lazy::new(|| {
        vec![
            (
                AgentHarness::ClaudeCode,
                word_regex(r"claude(?:[\s_-]*code)?|1"),
            ),
            (AgentHarness::Cursor, word_regex(r"cursor|2")),
            (
                AgentHarness::Codex,
                word_regex(r"(?:openai[\s_-]*)?codex|3"),
            ),
            (
                AgentHarness::OpenClaw,
                word_regex(r"open[\s_-]*claw|openclaw|4"),
            ),
            (
                AgentHarness::Hermes,
                word_regex(r"hermes(?:[\s_-]*agent)?|5"),
            ),
        ]
    });

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if ALL_RE.is_match(trimmed) {
        return Ok(AgentHarness::ALL.to_vec());
    }

    let mut selected = Vec::new();
    for (agent, pattern) in AGENT_RE.iter() {
        if pattern.is_match(trimmed) {
            push_unique_agent(&mut selected, *agent);
        }
    }

    if selected.is_empty() && NONE_RE.is_match(trimmed) {
        return Ok(Vec::new());
    }

    if selected.is_empty() {
        anyhow::bail!(
            "no agent found; mention Claude Code, Cursor, Codex, OpenClaw, Hermes, all, or none"
        );
    }
    Ok(selected)
}

fn word_regex(pattern: &str) -> Regex {
    Regex::new(&format!(r"(?i)(^|[^a-z0-9])(?:{pattern})([^a-z0-9]|$)"))
        .expect("agent selection regex must compile")
}

fn push_unique_agent(selected: &mut Vec<AgentHarness>, agent: AgentHarness) {
    if !selected.contains(&agent) {
        selected.push(agent);
    }
}

fn parse_agent_flags(values: &[String]) -> Result<Vec<AgentHarness>> {
    parse_agent_selection(&values.join(","))
}

fn choose_onboard_agents(
    p: &Palette,
    agent_flags: &[String],
    yes: bool,
) -> Result<Vec<AgentHarness>> {
    if !agent_flags.is_empty() {
        return parse_agent_flags(agent_flags);
    }

    if yes {
        return Ok(Vec::new());
    }

    if !std::io::stdin().is_terminal() {
        println!(
            "  {}",
            p.warn("non-interactive terminal: skipping agent wiring.")
        );
        println!(
            "  {}",
            p.dim("pass --agents claude-code,codex --yes to wire agents from scripts.")
        );
        return Ok(Vec::new());
    }

    println!("  Pick the agents that should share this memory:");
    for (idx, agent) in AgentHarness::ALL.iter().enumerate() {
        println!(
            "    {}. {:<12} {}",
            idx + 1,
            agent.label(),
            p.dim(agent.wiring_summary())
        );
    }
    println!(
        "  {}",
        p.dim("Enter numbers like 1,3,5, or type all / none. Default: none.")
    );

    loop {
        print!("  {} ", p.command("agents>"));
        io::stdout().flush().context("flush prompt")?;

        let mut line = String::new();
        io::stdin().read_line(&mut line).context("read prompt")?;
        match parse_agent_selection(&line) {
            Ok(selected) => return Ok(selected),
            Err(e) => println!("  {} {e}", p.warn("try again:")),
        }
    }
}

fn should_apply_wiring(p: &Palette, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        println!(
            "  {}",
            p.warn("non-interactive terminal: preview only; pass --yes to save.")
        );
        return Ok(false);
    }

    print!("  {} ", p.command("apply wiring? [Y/n]"));
    io::stdout().flush().context("flush prompt")?;
    let mut line = String::new();
    io::stdin().read_line(&mut line).context("read prompt")?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

fn build_wiring_plan(selected: &[AgentHarness], ctx: &WiringContext) -> Vec<WiringOp> {
    let mut ops = Vec::new();
    for harness in selected {
        match harness {
            AgentHarness::ClaudeCode => {
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.home.join(".claude/mcp_servers.json"),
                    description: "Claude Code MCP server config",
                    kind: WiringOpKind::WriteFileIfMissing {
                        content: CLAUDE_MCP,
                    },
                });
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.home.join(".claude/CLAUDE.md"),
                    description: "Claude Code user memory prompt block",
                    kind: WiringOpKind::AppendManagedBlock {
                        content: CLAUDE_BRAIN,
                    },
                });
            }
            AgentHarness::Cursor => {
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.project_dir.join(".cursor/mcp.json"),
                    description: "Cursor MCP server config",
                    kind: WiringOpKind::WriteFileIfMissing {
                        content: CURSOR_MCP,
                    },
                });
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.project_dir.join(".cursor/rules/brain.mdc"),
                    description: "Cursor alwaysApply memory rule",
                    kind: WiringOpKind::WriteManagedFile {
                        content: CURSOR_RULE,
                    },
                });
            }
            AgentHarness::Codex => {
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.home.join(".codex/config.toml"),
                    description: "Codex MCP server config block",
                    kind: WiringOpKind::AppendIfMissingText {
                        marker: "[mcp_servers.brain]",
                        content: CODEX_CONFIG,
                    },
                });
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.home.join(".codex/AGENTS.md"),
                    description: "Codex user memory prompt block",
                    kind: WiringOpKind::AppendManagedBlock {
                        content: CODEX_AGENTS,
                    },
                });
            }
            AgentHarness::OpenClaw => {
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.home.join(".openclaw/workspace/BRAIN.md"),
                    description: "OpenClaw workspace memory include",
                    kind: WiringOpKind::WriteManagedFile {
                        content: OPENCLAW_BRAIN,
                    },
                });
            }
            AgentHarness::Hermes => {
                ops.push(WiringOp {
                    harness: *harness,
                    path: ctx.project_dir.join("AGENTS.md"),
                    description: "Hermes project memory prompt block",
                    kind: WiringOpKind::AppendManagedBlock {
                        content: HERMES_BRAIN,
                    },
                });
            }
        }
    }
    ops
}

fn print_wiring_preview(p: &Palette, plan: &[WiringOp], reconfigure: bool) -> Result<()> {
    println!("  Review what will be written:");
    for op in plan {
        let action = op.planned_action(reconfigure)?;
        println!(
            "  - {:<20} {}",
            if action.is_write() {
                p.ok(action.label())
            } else {
                p.dim(action.label())
            },
            op.path.display()
        );
        println!("    {}", p.dim(op.description));
    }
    Ok(())
}

fn apply_wiring_plan(plan: &[WiringOp], reconfigure: bool) -> Result<Vec<PlannedAction>> {
    plan.iter().map(|op| op.apply(reconfigure)).collect()
}

impl WiringOp {
    fn planned_action(&self, reconfigure: bool) -> Result<PlannedAction> {
        match &self.kind {
            WiringOpKind::WriteFileIfMissing { .. } => {
                if self.path.exists() {
                    Ok(PlannedAction::SkipExists)
                } else {
                    Ok(PlannedAction::Create)
                }
            }
            WiringOpKind::WriteManagedFile { .. } => {
                if self.path.exists() {
                    if reconfigure {
                        Ok(PlannedAction::Replace)
                    } else {
                        Ok(PlannedAction::SkipAlreadyConfigured)
                    }
                } else {
                    Ok(PlannedAction::Create)
                }
            }
            WiringOpKind::AppendManagedBlock { .. } => {
                let text = read_optional_to_string(&self.path)?;
                if text.contains("BRAIN:START") {
                    if reconfigure {
                        Ok(PlannedAction::Replace)
                    } else {
                        Ok(PlannedAction::SkipAlreadyConfigured)
                    }
                } else if text.is_empty() {
                    Ok(PlannedAction::Create)
                } else {
                    Ok(PlannedAction::Append)
                }
            }
            WiringOpKind::AppendIfMissingText { marker, .. } => {
                let text = read_optional_to_string(&self.path)?;
                if text.contains(marker) {
                    Ok(PlannedAction::SkipAlreadyConfigured)
                } else if text.is_empty() {
                    Ok(PlannedAction::Create)
                } else {
                    Ok(PlannedAction::Append)
                }
            }
        }
    }

    fn apply(&self, reconfigure: bool) -> Result<PlannedAction> {
        let planned = self.planned_action(reconfigure)?;
        if !planned.is_write() {
            return Ok(planned);
        }

        match &self.kind {
            WiringOpKind::WriteFileIfMissing { content }
            | WiringOpKind::WriteManagedFile { content } => {
                write_file(&self.path, content)?;
            }
            WiringOpKind::AppendManagedBlock { content } => {
                let current = read_optional_to_string(&self.path)?;
                let next = if current.contains("BRAIN:START") {
                    replace_managed_block(&current, content).with_context(|| {
                        format!(
                            "{} has BRAIN:START without a matching BRAIN:END",
                            self.path.display()
                        )
                    })?
                } else {
                    append_managed_block(&current, content)
                };
                write_file(&self.path, &next)?;
            }
            WiringOpKind::AppendIfMissingText { content, .. } => {
                let current = read_optional_to_string(&self.path)?;
                let next = append_text(&current, content);
                write_file(&self.path, &next)?;
            }
        }

        Ok(planned)
    }
}

fn append_text(current: &str, content: &str) -> String {
    let mut next = current.to_string();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !next.trim().is_empty() {
        next.push('\n');
    }
    next.push_str(content.trim_end());
    next.push('\n');
    next
}

fn append_managed_block(current: &str, content: &str) -> String {
    append_text(current, &managed_block(content))
}

fn managed_block(content: &str) -> String {
    format!(
        "<!-- BRAIN:START - managed by brain onboard -->\n{}\n<!-- BRAIN:END -->",
        content.trim_end()
    )
}

fn replace_managed_block(current: &str, content: &str) -> Option<String> {
    let lines: Vec<&str> = current.lines().collect();
    let start = lines.iter().position(|line| line.contains("BRAIN:START"))?;
    let end = lines[start..]
        .iter()
        .position(|line| line.contains("BRAIN:END"))
        .map(|offset| start + offset)?;

    let mut next = String::new();
    for line in &lines[..start] {
        next.push_str(line);
        next.push('\n');
    }
    if !next.trim().is_empty() && !next.ends_with("\n\n") {
        next.push('\n');
    }

    next.push_str(&managed_block(content));
    next.push('\n');

    let tail = &lines[end + 1..];
    if !tail.is_empty() {
        next.push('\n');
        for line in tail {
            next.push_str(line);
            next.push('\n');
        }
    }

    Some(next)
}

fn read_optional_to_string(path: &PathBuf) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn write_file(path: &PathBuf, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .context("current directory")?
            .join(path))
    }
}

fn cmd_push(brain_dir: &PathBuf, remote: Option<&str>) -> Result<()> {
    let brain = open_or_fail(brain_dir)?;
    match brain.push(remote) {
        Ok(out) => {
            println!("Pushed.");
            if !out.is_empty() {
                for line in out.lines() {
                    println!("  {line}");
                }
            }
            Ok(())
        }
        Err(e) => {
            // brain doesn't ship a remote by default; guide the user.
            anyhow::bail!(
                "{}\n\nNo remote? Add one with:\n  brain remote add origin <url>",
                e
            )
        }
    }
}

fn cmd_pull(brain_dir: &PathBuf, remote: Option<&str>) -> Result<()> {
    let brain = open_or_fail(brain_dir)?;
    match brain.pull(remote) {
        Ok(out) => {
            println!("Pulled.");
            if !out.is_empty() {
                for line in out.lines() {
                    println!("  {line}");
                }
            }
            Ok(())
        }
        Err(e) => {
            anyhow::bail!(
                "{}\n\nNo remote? Add one with:\n  brain remote add origin <url>",
                e
            )
        }
    }
}

fn cmd_remote(brain_dir: &PathBuf, cmd: RemoteCmd) -> Result<()> {
    let brain = open_or_fail(brain_dir)?;
    match cmd {
        RemoteCmd::Add { name, url } => {
            brain.remote_add(&name, &url)?;
            println!("Added remote {name} -> {url}");
            Ok(())
        }
        RemoteCmd::List => {
            let remotes = brain.remote_list()?;
            if remotes.is_empty() {
                println!("No remotes configured. Add one with:\n  brain remote add origin <url>");
                return Ok(());
            }
            for (name, url) in remotes {
                println!("  {name:<10} {url}");
            }
            Ok(())
        }
    }
}

fn resolve_brain_dir(flag: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = flag {
        return Ok(p);
    }
    Ok(resolve_home_dir()?.join(".brain"))
}

fn resolve_home_dir() -> Result<PathBuf> {
    let home =
        std::env::var_os("HOME").context("HOME env var not set; pass --brain-dir explicitly")?;
    Ok(PathBuf::from(home))
}

fn cmd_init(brain_dir: &PathBuf) -> Result<()> {
    LocalBrain::init(brain_dir).with_context(|| format!("init at {}", brain_dir.display()))?;
    println!("Brain ready.");
    println!("  {}", brain_dir.display());
    Ok(())
}

/// Open the brain. For WRITE commands only (`note`), auto-create on
/// NotFound — nothing else. For READ commands, call `open_or_fail` instead
/// so a typo in --brain-dir doesn't silently create a new empty brain.
///
/// Auto-init is deliberately narrow: it does NOT fire for permission errors,
/// corrupt repos, or git repos that aren't brains (NotABrain). Those bubble
/// up with clear messages so the user fixes the path instead of getting a
/// surprise new brain.
fn open_or_init_for_write(brain_dir: &PathBuf) -> Result<LocalBrain> {
    use std::time::{Duration, Instant};

    // Multi-process race window. Scenarios we need to absorb:
    //   - two `brain note` invocations fire at the same fresh path; both see
    //     NotFound and both call init. One wins (AlreadyExists for the loser).
    //   - libgit2 holds `.git/config.lock` mid-init. Loser's init returns
    //     ErrorCode::Locked.
    //   - winner has created `.git/` but not yet written the bootstrap
    //     commit. Loser's open sees an unborn HEAD → NotABrain.
    //
    // Retry with exponential backoff for up to ~2s. That's way longer than
    // any real init on an SSD (~50ms) but still short enough that a genuinely
    // broken path fails within a human-tolerable window.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut delay = Duration::from_millis(25);

    loop {
        match LocalBrain::open(brain_dir) {
            Ok(b) => return Ok(b),
            Err(e) if e.is_not_found() => match LocalBrain::init(brain_dir) {
                Ok(b) => return Ok(b),
                Err(e) if e.is_transient_init_race() && Instant::now() < deadline => {
                    std::thread::sleep(delay);
                    delay = std::cmp::min(delay * 2, Duration::from_millis(250));
                    continue;
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("init at {}", brain_dir.display()));
                }
            },
            Err(e) if e.is_transient_init_race() && Instant::now() < deadline => {
                std::thread::sleep(delay);
                delay = std::cmp::min(delay * 2, Duration::from_millis(250));
                continue;
            }
            Err(e) => {
                return Err(e).with_context(|| format!("open at {}", brain_dir.display()));
            }
        }
    }
}

/// Open an existing brain. Does NOT auto-init. Used by read commands
/// (`log`, `ask`, `doctor`) so a typo in --brain-dir fails loudly instead
/// of silently creating an empty brain the user didn't ask for.
fn open_or_fail(brain_dir: &PathBuf) -> Result<LocalBrain> {
    match LocalBrain::open(brain_dir) {
        Ok(b) => Ok(b),
        Err(e) if e.is_not_found() => {
            anyhow::bail!(
                "No brain at {}.\nCreate one with: brain note \"anything\"",
                brain_dir.display()
            )
        }
        Err(e) => Err(e).with_context(|| format!("open at {}", brain_dir.display())),
    }
}

fn cmd_note(brain_dir: &PathBuf, text: String) -> Result<()> {
    let brain = open_or_init_for_write(brain_dir)?;
    match brain.observe_summary(text.clone()) {
        Ok(ev) => {
            if ev.was_idempotent_replay {
                println!("Already saved.");
            } else {
                println!("Saved.");
            }
            println!("  {}", one_line(&text, 120));
            Ok(())
        }
        // Secret-bearing notes are rejected BEFORE they hit git. We print
        // only the pattern name — never echo the note text back, which
        // would defeat the prefilter by logging the secret to the terminal.
        Err(ref e) if e.secret_rejection().is_some() => {
            let pattern = e.secret_rejection().unwrap();
            anyhow::bail!(
                "Refused: this note looks like it contains a secret ({pattern}).\nNot saved. The note text is not echoed back."
            )
        }
        Err(e) => Err(e).context("save note"),
    }
}

fn cmd_log(brain_dir: &PathBuf) -> Result<()> {
    let brain = open_or_fail(brain_dir)?;
    let events = brain.log(20).context("read notes")?;
    if events.is_empty() {
        println!("No notes yet. Try: brain note \"anything\"");
        return Ok(());
    }
    println!("Recent notes:");
    for ev in events {
        let text = ev.payload.summary_line(80);
        println!("  - {}", text);
    }
    Ok(())
}

fn cmd_ask(brain_dir: &PathBuf, query: String) -> Result<()> {
    // FTS5 has no "match all" syntax — a bare `*` isn't a valid prefix
    // query (it needs at least one preceding character) and would
    // surface as a rusqlite error. Redirect to `brain log` for users
    // reaching for "show me everything."
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed == "*" {
        println!("To list every note, use: brain log");
        println!("`ask` takes a word or phrase (e.g. brain ask \"fastapi\").");
        return Ok(());
    }
    let brain = open_or_fail(brain_dir)?;
    let hits = brain.search(&query, 5).context("search")?;
    if hits.is_empty() {
        println!("No matches for {:?}.", query);
        return Ok(());
    }
    println!("Matches:");
    for ev in hits {
        let text = ev.payload.summary_line(80);
        println!("  - {}", text);
    }
    Ok(())
}

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if flat.chars().count() > max {
        flat.chars()
            .take(max - 1)
            .chain(std::iter::once('…'))
            .collect()
    } else {
        flat
    }
}

fn cmd_serve(brain_dir: &PathBuf, mcp: bool) -> Result<()> {
    if !mcp {
        anyhow::bail!("brain serve requires --mcp in v0.1");
    }
    let brain =
        LocalBrain::open(brain_dir).with_context(|| format!("open at {}", brain_dir.display()))?;
    // stdio MCP must not log to stdout (would corrupt the JSON-RPC stream).
    // tracing_subscriber is already set to stderr by default.
    eprintln!(
        "brain-mcp: stdio server starting at {}",
        brain.path().display()
    );
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            brain_mcp::serve_stdio(brain)
                .await
                .map_err(|e| anyhow::anyhow!("mcp server: {e}"))
        })
}

fn cmd_tui(brain_dir: &PathBuf) -> Result<()> {
    // TUI owns the terminal in raw+alt-screen mode. Open-or-init so a
    // brand-new user can launch `brain tui` against an empty path and
    // land inside the app instead of hitting a `no brain here` error.
    let brain = open_or_init_for_write(brain_dir)?;
    brain_tui::run(brain)
}

fn cmd_doctor(brain_dir: &PathBuf, deep: bool) -> Result<()> {
    let brain = open_or_fail(brain_dir)?;

    if deep {
        // Wipe the derived index and replay the entire journal from git.
        // Fixes divergence that the on-open catch-up can't detect (equal
        // event counts with different contents, schema-version bumps,
        // FTS5 corruption). Cost: O(N) git reads + N inserts; fine at
        // personal-brain scale, slow at 100k+.
        let n = brain.rebuild_index().context("rebuild index")?;
        println!(
            "Rebuilt index from git: {} event{} replayed.",
            n,
            if n == 1 { "" } else { "s" }
        );
    }

    let r = brain.doctor().context("doctor")?;

    if r.issues.is_empty() {
        let index_hint = format!(" ({} indexed)", r.indexed_event_count);
        println!(
            "Ready. {} note{}{} at {}",
            r.event_count,
            if r.event_count == 1 { "" } else { "s" },
            index_hint,
            r.path.display()
        );
        if !r.warnings.is_empty() {
            println!("Warnings:");
            for warning in &r.warnings {
                println!("  - {}", warning);
            }
        }
    } else {
        println!("Problems found:");
        for issue in &r.issues {
            println!("  - {}", issue);
        }
        anyhow::bail!("{} issue(s)", r.issues.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_agent_names_numbers_and_deduplicates() {
        let selected = parse_agent_selection("1,codex hermes,claude").unwrap();
        assert_eq!(
            selected,
            vec![
                AgentHarness::ClaudeCode,
                AgentHarness::Codex,
                AgentHarness::Hermes,
            ]
        );
    }

    #[test]
    fn parses_display_names_with_spaces() {
        let selected = parse_agent_selection("Claude Code, OpenClaw, Hermes Agent").unwrap();
        assert_eq!(
            selected,
            vec![
                AgentHarness::ClaudeCode,
                AgentHarness::OpenClaw,
                AgentHarness::Hermes,
            ]
        );
    }

    #[test]
    fn parses_agent_words_inside_sentences() {
        let selected = parse_agent_selection("wire claude code and cursor for me").unwrap();
        assert_eq!(
            selected,
            vec![AgentHarness::ClaudeCode, AgentHarness::Cursor]
        );
    }

    #[test]
    fn parses_agent_all_and_none() {
        assert_eq!(
            parse_agent_selection("all").unwrap(),
            AgentHarness::ALL.to_vec()
        );
        assert!(parse_agent_selection("none").unwrap().is_empty());
        assert!(parse_agent_selection("").unwrap().is_empty());
    }

    #[test]
    fn replaces_existing_managed_block() {
        let current = "\
before
<!-- BRAIN:START - managed by brain install.sh -->
old block
<!-- BRAIN:END -->
after
";

        let next = replace_managed_block(current, "new block").unwrap();
        assert!(next.contains("before"));
        assert!(next.contains("new block"));
        assert!(next.contains("after"));
        assert!(!next.contains("old block"));
    }

    #[test]
    fn wiring_plan_writes_and_then_skips_idempotently() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        let ctx = WiringContext {
            home: home.path().to_path_buf(),
            project_dir: project.path().to_path_buf(),
        };
        let agents = AgentHarness::ALL.to_vec();
        let plan = build_wiring_plan(&agents, &ctx);

        let first = apply_wiring_plan(&plan, false).unwrap();
        assert!(first.iter().all(|action| action.is_write()));
        assert!(home.path().join(".claude/mcp_servers.json").exists());
        assert!(home.path().join(".codex/config.toml").exists());
        assert!(home.path().join(".openclaw/workspace/BRAIN.md").exists());
        assert!(project.path().join(".cursor/rules/brain.mdc").exists());
        assert!(project.path().join("AGENTS.md").exists());

        let second = apply_wiring_plan(&plan, false).unwrap();
        assert!(second.iter().all(|action| !action.is_write()));

        let third = apply_wiring_plan(&plan, true).unwrap();
        assert!(third.contains(&PlannedAction::Replace));
    }
}
