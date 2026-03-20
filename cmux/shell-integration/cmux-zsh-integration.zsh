#!/usr/bin/env zsh
# cmux zsh integration — CWD reporting, git branch, PR polling, port kicks.
#
# Sourced automatically when CMUX_SOCKET is set (injected by cmux into
# terminal environment). Can also be sourced manually from ~/.zshrc.
#
# Protocol: V1 text lines over the cmux Unix socket.

# Bail if not running inside cmux
[[ -n "$CMUX_SOCKET" ]] || return 0

# ── Socket transport ──────────────────────────────────────────────────
_cmux_send() {
  local msg="$1"
  if command -v socat >/dev/null 2>&1; then
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

# ── CWD reporting ────────────────────────────────────────────────────
_cmux_report_pwd() {
  _cmux_send_fire_forget "report_pwd \"$PWD\" $(_cmux_flags)"
}

# ── Git branch (async to avoid blocking prompt) ──────────────────────
_cmux_git_branch=""
_cmux_git_dirty=""

_cmux_update_git_branch() {
  local branch
  branch=$(git symbolic-ref --short HEAD 2>/dev/null || git rev-parse --short HEAD 2>/dev/null)
  if [[ -n "$branch" ]]; then
    _cmux_git_branch="$branch"
    # Quick dirty check (index only, skip untracked for speed)
    if git diff-index --quiet HEAD -- 2>/dev/null; then
      _cmux_git_dirty=""
    else
      _cmux_git_dirty="*"
    fi
    _cmux_send_fire_forget "report_git_branch ${branch}${_cmux_git_dirty} $(_cmux_flags)"
  elif [[ -n "$_cmux_git_branch" ]]; then
    _cmux_git_branch=""
    _cmux_git_dirty=""
    _cmux_send_fire_forget "clear_git_branch $(_cmux_flags)"
  fi
}

# ── PR status polling (background, every 45s) ────────────────────────
_cmux_pr_poll_pid=""

_cmux_start_pr_poll() {
  # Kill previous poll if running
  [[ -n "$_cmux_pr_poll_pid" ]] && kill "$_cmux_pr_poll_pid" 2>/dev/null

  (
    while true; do
      sleep 45
      if command -v gh >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        local pr_json
        pr_json=$(timeout 10 gh pr view --json state,statusCheckRollup 2>/dev/null)
        if [[ -n "$pr_json" ]]; then
          local pr_state
          pr_state=$(echo "$pr_json" | grep -o '"state":"[^"]*"' | head -1 | cut -d'"' -f4)
          if [[ -n "$pr_state" ]]; then
            _cmux_send "report_pr $pr_state $(_cmux_flags)" >/dev/null 2>&1
          fi
        fi
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

# ── Hook into zsh prompt lifecycle ───────────────────────────────────
_cmux_precmd() {
  _cmux_report_pwd
  _cmux_update_git_branch
  _cmux_report_prompt
}

_cmux_preexec() {
  _cmux_report_running
}

# Register hooks (idempotent — won't double-register)
autoload -Uz add-zsh-hook
add-zsh-hook precmd  _cmux_precmd
add-zsh-hook preexec _cmux_preexec

# ── Initial reports ──────────────────────────────────────────────────
_cmux_report_pwd
_cmux_report_tty
_cmux_update_git_branch
_cmux_ports_kick

# Start PR polling if gh is available
if command -v gh >/dev/null 2>&1; then
  _cmux_start_pr_poll
fi
