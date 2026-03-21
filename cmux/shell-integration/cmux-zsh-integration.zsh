#!/usr/bin/env zsh
# cmux zsh integration — CWD reporting, git branch, PR polling, port kicks,
# scrollback restoration, async git HEAD watcher, semantic prompt markers,
# fast git HEAD resolution, smart PR polling, WINCH guard, process cleanup.
#
# Sourced automatically via ZDOTDIR bootstrap (.zshenv) when CMUX_SOCKET is
# set (injected by cmux into terminal environment). Can also be sourced
# manually from ~/.zshrc.
#
# Protocol: V1 text lines over the cmux Unix socket.

# Bail if not running inside cmux
[[ -n "$CMUX_SOCKET" ]] || return 0

# Guard against double-sourcing
[[ -n "$_CMUX_ZSH_INTEGRATION_LOADED" ]] && return 0
_CMUX_ZSH_INTEGRATION_LOADED=1

# ── Socket transport ──────────────────────────────────────────────────
# Try ncat (nmap's netcat, reliable for Unix sockets), then socat, then nc.
_cmux_send() {
  local msg="$1"
  if command -v ncat >/dev/null 2>&1; then
    echo "$msg" | ncat -U "$CMUX_SOCKET" 2>/dev/null
  elif command -v socat >/dev/null 2>&1; then
    echo "$msg" | socat - UNIX-CONNECT:"$CMUX_SOCKET" 2>/dev/null
  elif command -v nc >/dev/null 2>&1; then
    echo "$msg" | nc -U "$CMUX_SOCKET" -w 1 2>/dev/null
  fi
}

_cmux_send_fire_forget() {
  _cmux_send "$1" >/dev/null 2>&1 &!
}

# ── Workspace / panel identifiers ────────────────────────────────────
_cmux_flags() {
  local flags=""
  [[ -n "$CMUX_WORKSPACE_ID" ]] && flags="$flags --tab=$CMUX_WORKSPACE_ID"
  [[ -n "$CMUX_PANEL_ID" ]]     && flags="$flags --panel=$CMUX_PANEL_ID"
  echo "$flags"
}

# ── Scrollback restoration ────────────────────────────────────────────
# On session restore cmux writes saved scrollback to a temp file and sets
# CMUX_RESTORE_SCROLLBACK_FILE. We replay it once, then delete the file.
_cmux_restore_scrollback_once() {
  [[ -z "$CMUX_RESTORE_SCROLLBACK_FILE" ]] && return
  local f="$CMUX_RESTORE_SCROLLBACK_FILE"
  unset CMUX_RESTORE_SCROLLBACK_FILE
  if [[ -f "$f" ]]; then
    cat "$f" 2>/dev/null
    rm -f "$f" 2>/dev/null
  fi
}

# ── CWD reporting ────────────────────────────────────────────────────
_cmux_report_pwd() {
  _cmux_send_fire_forget "report_pwd \"$PWD\" $(_cmux_flags)"
}

# ── Fast git HEAD resolution ─────────────────────────────────────────
# Reads .git/HEAD directly without invoking `git` for speed on large repos.
# Handles both regular repos and git worktrees (.git as file with gitdir pointer).
_cmux_git_resolve_head_path() {
  local dir="$PWD"
  while [[ "$dir" != "/" ]]; do
    if [[ -f "$dir/.git/HEAD" ]]; then
      echo "$dir/.git/HEAD"
      return 0
    elif [[ -f "$dir/.git" ]]; then
      # Worktree: .git is a file containing "gitdir: <path>"
      local gitdir
      gitdir=$(< "$dir/.git")
      gitdir="${gitdir#gitdir: }"
      # Resolve relative paths
      [[ "$gitdir" != /* ]] && gitdir="$dir/$gitdir"
      if [[ -f "$gitdir/HEAD" ]]; then
        echo "$gitdir/HEAD"
        return 0
      fi
    fi
    dir="${dir:h}"
  done
  return 1
}

# Read branch name from a HEAD file without forking git.
_cmux_git_read_branch_from_head() {
  local head_file="$1"
  [[ -f "$head_file" ]] || return 1
  local content
  content=$(< "$head_file" 2>/dev/null) || return 1
  # "ref: refs/heads/<branch>"
  if [[ "$content" == ref:\ refs/heads/* ]]; then
    echo "${content#ref: refs/heads/}"
    return 0
  fi
  # Detached HEAD — short hash
  echo "${content:0:8}"
  return 0
}

# ── Git branch (fast path + fallback) ────────────────────────────────
_cmux_git_branch=""
_cmux_git_dirty=""
_cmux_git_head_path=""

_cmux_update_git_branch() {
  # Try fast path first (no fork)
  if [[ -z "$_cmux_git_head_path" ]] || [[ ! -f "$_cmux_git_head_path" ]]; then
    _cmux_git_head_path=$(_cmux_git_resolve_head_path 2>/dev/null)
  fi

  local branch=""
  if [[ -n "$_cmux_git_head_path" ]]; then
    branch=$(_cmux_git_read_branch_from_head "$_cmux_git_head_path")
  fi

  # Fallback to git if fast path fails
  if [[ -z "$branch" ]]; then
    branch=$(git symbolic-ref --short HEAD 2>/dev/null \
             || git rev-parse --short HEAD 2>/dev/null)
  fi

  if [[ -n "$branch" ]]; then
    _cmux_git_branch="$branch"
    # Quick dirty check (index only, skip untracked for speed)
    if git diff-index --quiet HEAD -- 2>/dev/null; then
      _cmux_git_dirty=""
    else
      _cmux_git_dirty="*"
    fi
    _cmux_send_fire_forget \
      "report_git_branch ${branch}${_cmux_git_dirty} $(_cmux_flags)"
  elif [[ -n "$_cmux_git_branch" ]]; then
    _cmux_git_branch=""
    _cmux_git_dirty=""
    _cmux_git_head_path=""
    _cmux_send_fire_forget "clear_git_branch $(_cmux_flags)"
  fi
}

# Invalidate cached HEAD path when CWD changes
_cmux_chpwd() {
  _cmux_git_head_path=""
}
autoload -Uz add-zsh-hook
add-zsh-hook chpwd _cmux_chpwd

# ── Async git HEAD watcher ────────────────────────────────────────────
# While a command is running, poll .git/HEAD every 2s so branch switches
# (e.g. during `git rebase` or `git checkout`) are reflected immediately.
_cmux_git_watcher_pid=""

_cmux_start_git_watcher() {
  local head_file="$_cmux_git_head_path"
  if [[ -z "$head_file" ]]; then
    local git_dir
    git_dir=$(git rev-parse --git-dir 2>/dev/null) || return
    head_file="$git_dir/HEAD"
  fi
  [[ -f "$head_file" ]] || return

  (
    local last_head
    last_head=$(< "$head_file" 2>/dev/null)
    while true; do
      sleep 2
      local cur_head
      cur_head=$(< "$head_file" 2>/dev/null)
      if [[ "$cur_head" != "$last_head" ]]; then
        last_head="$cur_head"
        local branch="${cur_head#ref: refs/heads/}"
        if [[ "$branch" != "$cur_head" && -n "$branch" ]]; then
          _cmux_send "report_git_branch $branch $(_cmux_flags)" >/dev/null 2>&1
        fi
      fi
    done
  ) &!
  _cmux_git_watcher_pid=$!
}

_cmux_stop_git_watcher() {
  if [[ -n "$_cmux_git_watcher_pid" ]]; then
    kill "$_cmux_git_watcher_pid" 2>/dev/null
    _cmux_git_watcher_pid=""
  fi
}

# ── PR status polling (background, every 45s) ────────────────────────
_cmux_pr_poll_pid=""
_cmux_pr_last_status=""

# Extract owner/repo from git remote for smarter PR queries
_cmux_github_repo_slug() {
  local remote_url
  remote_url=$(git remote get-url origin 2>/dev/null) || return 1
  # Handle SSH: git@github.com:owner/repo.git
  if [[ "$remote_url" == git@github.com:* ]]; then
    local slug="${remote_url#git@github.com:}"
    echo "${slug%.git}"
    return 0
  fi
  # Handle HTTPS: https://github.com/owner/repo.git
  if [[ "$remote_url" == https://github.com/* ]]; then
    local slug="${remote_url#https://github.com/}"
    echo "${slug%.git}"
    return 0
  fi
  return 1
}

# Detect whether gh output indicates "no PR" vs transient failure
_cmux_pr_output_indicates_no_pr() {
  local output="$1" exit_code="$2"
  # gh exits 1 with "no pull requests found" when there's genuinely no PR
  [[ "$exit_code" -ne 0 ]] && [[ "$output" == *"no pull requests"* ]] && return 0
  return 1
}

_cmux_start_pr_poll() {
  [[ -n "$_cmux_pr_poll_pid" ]] && kill "$_cmux_pr_poll_pid" 2>/dev/null

  (
    while true; do
      sleep 45
      if command -v gh >/dev/null 2>&1 \
         && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        local pr_output pr_exit
        pr_output=$(timeout 20 gh pr view --json state,statusCheckRollup 2>&1)
        pr_exit=$?
        if [[ "$pr_exit" -eq 0 && -n "$pr_output" ]]; then
          local pr_state
          pr_state=$(echo "$pr_output" \
                     | grep -o '"state":"[^"]*"' | head -1 | cut -d'"' -f4)
          if [[ -n "$pr_state" ]]; then
            _cmux_pr_last_status="$pr_state"
            _cmux_send "report_pr $pr_state $(_cmux_flags)" >/dev/null 2>&1
          fi
        elif _cmux_pr_output_indicates_no_pr "$pr_output" "$pr_exit"; then
          # Genuinely no PR — clear badge
          if [[ -n "$_cmux_pr_last_status" ]]; then
            _cmux_pr_last_status=""
            _cmux_send "clear_pr $(_cmux_flags)" >/dev/null 2>&1
          fi
        fi
        # Transient failure — preserve last known status
      fi
    done
  ) &!
  _cmux_pr_poll_pid=$!
}

# ── TTY reporting ────────────────────────────────────────────────────
_cmux_report_tty() {
  local tty_name
  tty_name=$(tty 2>/dev/null)
  [[ -n "$tty_name" ]] && _cmux_send_fire_forget "report_tty $tty_name $(_cmux_flags)"
}

# ── Port scanning kick ──────────────────────────────────────────────
_cmux_ports_kick() {
  _cmux_send_fire_forget "ports_kick"
}

# ── Shell state reporting ────────────────────────────────────────────
_cmux_report_prompt() {
  _cmux_send_fire_forget "report_shell_state prompt $(_cmux_flags)"
}

_cmux_report_running() {
  _cmux_send_fire_forget "report_shell_state running $(_cmux_flags)"
}

# ── Semantic prompt markers (OSC 133) ─────────────────────────────────
_cmux_osc133_prompt_start() {
  printf '\e]133;A\a'
}

_cmux_osc133_command_start() {
  printf '\e]133;C\a'
}

_cmux_osc133_command_end() {
  printf '\e]133;D;%s\a' "$1"
}

# ── WINCH signal guard ───────────────────────────────────────────────
# Install a WINCH trap to prevent prompt corruption on terminal resize.
# Without this, zsh may redraw the prompt mid-resize causing glitches.
TRAPWINCH() {
  # No-op: absorb the signal to prevent zle redraw during resize.
  # Ghostty handles resize internally.
  :
}

# ── Process tree cleanup ─────────────────────────────────────────────
# Kill background child processes (git watcher, PR poll) on shell exit.
_cmux_cleanup() {
  _cmux_stop_git_watcher
  [[ -n "$_cmux_pr_poll_pid" ]] && kill "$_cmux_pr_poll_pid" 2>/dev/null
}
add-zsh-hook zshexit _cmux_cleanup

# ── Hook into zsh prompt lifecycle ───────────────────────────────────
_cmux_last_exit=0

_cmux_precmd() {
  local exit_code=$?
  _cmux_last_exit=$exit_code

  # Stop git watcher — command finished, prompt is back
  _cmux_stop_git_watcher

  # Semantic: mark end of previous command output
  _cmux_osc133_command_end "$exit_code"

  # Report state to cmux
  _cmux_report_pwd
  _cmux_update_git_branch
  _cmux_report_prompt

  # Semantic: mark start of prompt
  _cmux_osc133_prompt_start
}

_cmux_preexec() {
  # Semantic: mark start of command output
  _cmux_osc133_command_start

  _cmux_report_running

  # Start async git HEAD watcher while command runs
  _cmux_start_git_watcher
}

# Register hooks (idempotent — won't double-register)
add-zsh-hook precmd  _cmux_precmd
add-zsh-hook preexec _cmux_preexec

# ── Initial reports ──────────────────────────────────────────────────
_cmux_restore_scrollback_once
_cmux_report_pwd
_cmux_report_tty
_cmux_update_git_branch
_cmux_ports_kick

# Start PR polling if gh is available
if command -v gh >/dev/null 2>&1; then
  _cmux_start_pr_poll
fi
