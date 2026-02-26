// Package welcome renders the initial greeting screen for the CodeFlow CLI.
//
// The welcome screen displays a Unicode box-drawing frame with the CODEFLOW
// block logo, version info, project status, active work, context-aware quick
// start commands, and optional update notifications. All output is written to
// an [io.Writer] for testability.
//
// Configuration uses the functional options pattern—no global state.
package welcome

import (
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"golang.org/x/term"
)

// boxWidth is the total width of the welcome screen box (including borders).
const boxWidth = 78

// innerWidth is the usable width inside the box borders.
const innerWidth = boxWidth - 2 // 76

// ANSI color codes.
const (
	ansiReset     = "\033[0m"
	ansiDim       = "\033[2m"
	ansiCyan      = "\033[36m"
	ansiGreen     = "\033[32m"
	ansiYellow    = "\033[33m"
	ansiRed       = "\033[31m"
	ansiBlue      = "\033[34m"
	ansiBoldWhite = "\033[1;37m"
	ansiBoldCyan  = "\033[1;36m"
	ansiDimWhite  = "\033[2;37m"
	ansiDimItalic = "\033[2;3m"
)

// Unicode box-drawing characters (double-line).
const (
	boxTopLeft     = "╔"
	boxTopRight    = "╗"
	boxBottomLeft  = "╚"
	boxBottomRight = "╝"
	boxHorizontal  = "═"
	boxVertical    = "║"
	boxLeftT       = "╠"
	boxRightT      = "╣"
)

// ASCII fallback box-drawing characters.
const (
	asciiTopLeft     = "+"
	asciiTopRight    = "+"
	asciiBottomLeft  = "+"
	asciiBottomRight = "+"
	asciiHorizontal  = "="
	asciiVertical    = "|"
	asciiLeftT       = "+"
	asciiRightT      = "+"
)

// boxChars holds the set of box-drawing characters to use.
type boxChars struct {
	topLeft     string
	topRight    string
	bottomLeft  string
	bottomRight string
	horizontal  string
	vertical    string
	leftT       string
	rightT      string
}

var unicodeBox = boxChars{
	topLeft: boxTopLeft, topRight: boxTopRight,
	bottomLeft: boxBottomLeft, bottomRight: boxBottomRight,
	horizontal: boxHorizontal, vertical: boxVertical,
	leftT: boxLeftT, rightT: boxRightT,
}

var asciiBox = boxChars{
	topLeft: asciiTopLeft, topRight: asciiTopRight,
	bottomLeft: asciiBottomLeft, bottomRight: asciiBottomRight,
	horizontal: asciiHorizontal, vertical: asciiVertical,
	leftT: asciiLeftT, rightT: asciiRightT,
}

// ActiveWork describes current in-progress work.
type ActiveWork struct {
	TaskID  string
	Title   string
	Branch  string
	Started time.Time
}

// QuickStartCmd is a command entry for the Quick Start section.
type QuickStartCmd struct {
	Command     string
	Description string
}

// ProjectInfo holds project name and status.
type ProjectInfo struct {
	Name   string
	Status string // "Active", "Warning", "Error"
}

// UpdateInfo describes an available update.
type UpdateInfo struct {
	Available  bool
	NewVersion string
}

// Config holds all welcome screen configuration. Use functional options
// (Option) to set fields—no global state, thread-safe.
type Config struct {
	// Version fields.
	CodeflowVersion   string
	ClaudeCodeVersion string

	// Project info.
	Project ProjectInfo

	// Active work (nil = no active work).
	ActiveWork *ActiveWork

	// Quick start commands (nil = auto-detect from state).
	QuickStart []QuickStartCmd

	// Update info.
	Update UpdateInfo

	// Display options.
	NoColor    bool
	Quiet      bool
	UseASCII   bool
	TermWidth  int // 0 = auto-detect

	// Writer for the output file descriptor (for isatty check).
	// Defaults to os.Stdout.
	OutputFd uintptr
}

// Option is a functional option for configuring the welcome screen.
type Option func(*Config)

// WithVersion sets the CodeFlow version string.
func WithVersion(v string) Option {
	return func(c *Config) { c.CodeflowVersion = v }
}

// WithClaudeCodeVersion sets the Claude Code version string.
func WithClaudeCodeVersion(v string) Option {
	return func(c *Config) { c.ClaudeCodeVersion = v }
}

// WithProject sets the project info.
func WithProject(name, status string) Option {
	return func(c *Config) {
		c.Project = ProjectInfo{Name: name, Status: status}
	}
}

// WithActiveWork sets the active work info.
func WithActiveWork(taskID, title, branch string, started time.Time) Option {
	return func(c *Config) {
		c.ActiveWork = &ActiveWork{
			TaskID:  taskID,
			Title:   title,
			Branch:  branch,
			Started: started,
		}
	}
}

// WithQuickStart sets custom quick start commands.
func WithQuickStart(cmds []QuickStartCmd) Option {
	return func(c *Config) { c.QuickStart = cmds }
}

// WithUpdate sets the update notification info.
func WithUpdate(newVersion string) Option {
	return func(c *Config) {
		c.Update = UpdateInfo{Available: true, NewVersion: newVersion}
	}
}

// WithNoColor disables ANSI color output.
func WithNoColor(v bool) Option {
	return func(c *Config) { c.NoColor = v }
}

// WithQuiet enables quiet mode (suppresses welcome screen).
func WithQuiet(v bool) Option {
	return func(c *Config) { c.Quiet = v }
}

// WithASCII forces ASCII box-drawing characters.
func WithASCII(v bool) Option {
	return func(c *Config) { c.UseASCII = v }
}

// WithTermWidth overrides the terminal width detection.
func WithTermWidth(w int) Option {
	return func(c *Config) { c.TermWidth = w }
}

// WithOutputFd sets the file descriptor for isatty detection.
func WithOutputFd(fd uintptr) Option {
	return func(c *Config) { c.OutputFd = fd }
}

// Show renders the welcome screen to the given writer.
func Show(w io.Writer, opts ...Option) {
	cfg := &Config{
		CodeflowVersion:   "dev",
		ClaudeCodeVersion: "unknown",
		Project:           ProjectInfo{Name: "codeflow", Status: "Active"},
		OutputFd:          os.Stdout.Fd(),
	}

	for _, opt := range opts {
		opt(cfg)
	}

	// Quiet mode: output nothing.
	if cfg.Quiet {
		return
	}

	// Detect terminal capabilities.
	isTerminal := term.IsTerminal(int(cfg.OutputFd))

	// NO_COLOR env var support.
	if _, ok := os.LookupEnv("NO_COLOR"); ok {
		cfg.NoColor = true
	}

	// Non-interactive: disable color.
	if !isTerminal {
		cfg.NoColor = true
	}

	// Detect terminal width if not overridden.
	termWidth := cfg.TermWidth
	if termWidth == 0 && isTerminal {
		if tw, _, err := term.GetSize(int(cfg.OutputFd)); err == nil && tw > 0 {
			termWidth = tw
		}
	}
	if termWidth == 0 {
		termWidth = 80 // Default assumption.
	}

	// Detect ASCII requirement from LANG/LC_ALL.
	if !cfg.UseASCII {
		cfg.UseASCII = !isUTF8Terminal()
	}

	// Select box characters.
	bc := unicodeBox
	if cfg.UseASCII {
		bc = asciiBox
	}

	r := &renderer{
		w:   w,
		cfg: cfg,
		bc:  bc,
		tw:  termWidth,
	}

	// Auto-detect quick start commands if not provided.
	if cfg.QuickStart == nil {
		cfg.QuickStart = defaultQuickStart(cfg.ActiveWork)
	}

	r.render()
}

// renderer handles the actual rendering of the welcome screen.
type renderer struct {
	w   io.Writer
	cfg *Config
	bc  boxChars
	tw  int // terminal width
}

func (r *renderer) render() {
	r.topBorder()
	r.emptyLine()

	// Logo section: full block logo or compact fallback.
	if r.tw >= boxWidth {
		r.blockLogo()
	} else {
		r.compactHeader()
	}

	r.emptyLine()
	r.tagline()
	r.claudeCodeVersion()
	r.emptyLine()
	r.separator()

	// Project section.
	r.emptyLine()
	r.projectSection()
	r.emptyLine()

	// Active work section.
	r.activeWorkSection()

	r.separator()

	// Quick start section.
	r.emptyLine()
	r.quickStartSection()
	r.emptyLine()

	// Update notification (conditional).
	if r.cfg.Update.Available {
		r.separator()
		r.emptyLine()
		r.updateSection()
		r.emptyLine()
	}

	// Footer prompt.
	r.promptLine()
	r.bottomBorder()
}

// --- Rendering helpers ---

func (r *renderer) color(code, text string) string {
	if r.cfg.NoColor {
		return text
	}
	return code + text + ansiReset
}

func (r *renderer) topBorder() {
	line := r.bc.topLeft + strings.Repeat(r.bc.horizontal, innerWidth) + r.bc.topRight
	fmt.Fprintln(r.w, r.color(ansiDimWhite, line))
}

func (r *renderer) bottomBorder() {
	line := r.bc.bottomLeft + strings.Repeat(r.bc.horizontal, innerWidth) + r.bc.bottomRight
	fmt.Fprintln(r.w, r.color(ansiDimWhite, line))
}

func (r *renderer) separator() {
	line := r.bc.leftT + strings.Repeat(r.bc.horizontal, innerWidth) + r.bc.rightT
	fmt.Fprintln(r.w, r.color(ansiDimWhite, line))
}

func (r *renderer) emptyLine() {
	fmt.Fprintln(r.w, r.color(ansiDimWhite, r.bc.vertical)+
		strings.Repeat(" ", innerWidth)+
		r.color(ansiDimWhite, r.bc.vertical))
}

// contentLine renders a line with content padded to fill the box width.
func (r *renderer) contentLine(content string) {
	// Calculate visible length (stripping ANSI codes).
	visLen := visibleLength(content)
	padding := innerWidth - 2 - visLen // 2 for left margin
	if padding < 0 {
		padding = 0
	}
	fmt.Fprintln(r.w,
		r.color(ansiDimWhite, r.bc.vertical)+
			"  "+content+strings.Repeat(" ", padding)+
			r.color(ansiDimWhite, r.bc.vertical))
}

// blockLogo renders the 6-line CODEFLOW block logo in cyan.
func (r *renderer) blockLogo() {
	logo := []string{
		`██████╗  ██████╗ ██████╗ ███████╗███████╗██╗      ██████╗ ██╗    ██╗`,
		`██╔════╝ ██╔═══██╗██╔══██╗██╔════╝██╔════╝██║     ██╔═══██╗██║    ██║`,
		`██║      ██║   ██║██║  ██║█████╗  █████╗  ██║     ██║   ██║██║ █╗ ██║`,
		`██║      ██║   ██║██║  ██║██╔══╝  ██╔══╝  ██║     ██║   ██║██║███╗██║`,
		`╚██████╗ ╚██████╔╝██████╔╝███████╗██║     ███████╗╚██████╔╝╚███╔███╔╝`,
		` ╚═════╝  ╚═════╝ ╚═════╝ ╚══════╝╚═╝     ╚══════╝ ╚═════╝  ╚══╝╚══╝`,
	}

	for _, line := range logo {
		r.contentLine(r.color(ansiCyan, line))
	}
}

// compactHeader renders a compact text header when terminal is too narrow.
func (r *renderer) compactHeader() {
	text := "CODEFLOW v" + r.cfg.CodeflowVersion
	r.contentLine(r.color(ansiBoldCyan, text))
}

func (r *renderer) tagline() {
	tagline := "AI-Powered Software Development Workflow"
	ver := "  v" + r.cfg.CodeflowVersion
	content := r.color(ansiDimItalic, tagline) + r.color(ansiDim, ver)
	r.contentLine(content)
}

func (r *renderer) claudeCodeVersion() {
	line := "Claude Code " + r.cfg.ClaudeCodeVersion
	r.contentLine(r.color(ansiDim, line))
}

func (r *renderer) projectSection() {
	nameLabel := r.color(ansiBoldWhite, "PROJECT:") + " " + r.cfg.Project.Name
	r.contentLine(nameLabel)

	statusColor := ansiGreen
	switch r.cfg.Project.Status {
	case "Warning":
		statusColor = ansiYellow
	case "Error":
		statusColor = ansiRed
	}
	statusLabel := r.color(ansiBoldWhite, "STATUS:") + "  " + r.color(statusColor, r.cfg.Project.Status)
	r.contentLine(statusLabel)
}

func (r *renderer) activeWorkSection() {
	if r.cfg.ActiveWork != nil {
		r.contentLine(r.color(ansiBoldWhite, "ACTIVE WORK:"))

		taskLine := "  \U0001F528 " +
			r.color(ansiCyan, r.cfg.ActiveWork.TaskID) +
			" " + `"` + r.cfg.ActiveWork.Title + `"`
		r.contentLine(taskLine)

		branchLine := "     Branch: " + r.color(ansiBlue, r.cfg.ActiveWork.Branch)
		r.contentLine(branchLine)

		startedLine := "     Started: " + formatTimeAgo(r.cfg.ActiveWork.Started)
		r.contentLine(startedLine)
		r.emptyLine()
	} else {
		r.contentLine(r.color(ansiBoldWhite, "NO ACTIVE WORK"))
		r.contentLine("Start with /cf-plan or pick up existing tasks")
		r.emptyLine()
	}
}

func (r *renderer) quickStartSection() {
	r.contentLine(r.color(ansiBoldWhite, "QUICK START:"))

	for _, cmd := range r.cfg.QuickStart {
		// Pad command to fixed width for alignment.
		paddedCmd := cmd.Command + strings.Repeat(" ", max(0, 20-len(cmd.Command)))
		line := "  " + r.color(ansiBoldCyan, paddedCmd) + " " + cmd.Description
		r.contentLine(line)
	}
}

func (r *renderer) updateSection() {
	line := r.color(ansiYellow, "\u26A0\uFE0F  UPDATE AVAILABLE: "+
		r.cfg.Update.NewVersion+" (run: codeflow update)")
	r.contentLine(line)
}

func (r *renderer) promptLine() {
	line := r.color(ansiDim, "Press Enter to continue, or 'q' to quit...")
	r.contentLine(line)
}

// --- Utility functions ---

// defaultQuickStart returns context-aware quick start commands.
func defaultQuickStart(aw *ActiveWork) []QuickStartCmd {
	if aw != nil {
		return []QuickStartCmd{
			{Command: "/cf-resume", Description: "Resume active work"},
			{Command: `/cf-plan "feature"`, Description: "Plan new feature"},
			{Command: "/cf-help", Description: "Show all commands"},
		}
	}
	return []QuickStartCmd{
		{Command: `/cf-plan "feature"`, Description: "Plan new feature"},
		{Command: `/cf-develop "task"`, Description: "Start development"},
		{Command: "/cf-help", Description: "Show all commands"},
	}
}

// formatTimeAgo returns a human-readable duration since the given time.
func formatTimeAgo(t time.Time) string {
	if t.IsZero() {
		return "unknown"
	}

	d := time.Since(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		m := int(d.Minutes())
		if m == 1 {
			return "1 minute ago"
		}
		return fmt.Sprintf("%d minutes ago", m)
	case d < 24*time.Hour:
		h := int(d.Hours())
		if h == 1 {
			return "1 hour ago"
		}
		return fmt.Sprintf("%d hours ago", h)
	default:
		days := int(d.Hours() / 24)
		if days == 1 {
			return "1 day ago"
		}
		return fmt.Sprintf("%d days ago", days)
	}
}

// visibleLength calculates the visible length of a string after stripping
// ANSI escape sequences.
func visibleLength(s string) int {
	inEscape := false
	length := 0
	for _, r := range s {
		if r == '\033' {
			inEscape = true
			continue
		}
		if inEscape {
			if r == 'm' {
				inEscape = false
			}
			continue
		}
		// Count rune width: most runes are 1 cell, but some Unicode
		// characters like box-drawing are also 1 cell.
		length++
	}
	return length
}

// isUTF8Terminal checks if the terminal supports UTF-8 by examining
// LANG and LC_ALL environment variables.
func isUTF8Terminal() bool {
	for _, env := range []string{"LC_ALL", "LANG"} {
		val := os.Getenv(env)
		if val == "" {
			continue
		}
		lower := strings.ToLower(val)
		if strings.Contains(lower, "utf-8") || strings.Contains(lower, "utf8") {
			return true
		}
	}
	// Default to UTF-8 on modern systems if no locale is set.
	return true
}

