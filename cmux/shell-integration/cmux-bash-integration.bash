#!/usr/bin/env bash
# cmux bash integration — CWD reporting, git branch, PR polling, port kicks,
# scrollback restoration, async git HEAD watcher, semantic prompt markers,
# fast git HEAD resolution, smart PR polling, PS0 preexec, process cleanup.
#
# Auto-injected via BASH_ENV when CMUX_SOCKET is set (injected by cmux into
# terminal environment). Can also be sourced manually from ~/.bashrc.
#
# Protocol: V1 text lines over the cmux Unix socket.

# Bail if not running inside cmux
[[ -n "$CMUX_SOCKET" ]] || return 0

# Guard against double-sourcing
[[ -n "$_CMUX_BASH_INTEGRATION_LOADED" ]] && return 0
_CMUX_BASH_INTEGRATION_LOADED=1

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

_cmux_send_bg() {
  _cmux_send "$1" >/dev/null 2>&1 &
  disown 2>/dev/null
}

# ── Workspace / panel identifiers ────────────────────────────────────
_cmux_flags() {
  local flags=""
  [[ -n "$CMUX_WORKSPACE_ID" ]] && flags="$flags --tab=$CMUX_WORKSPACE_ID"
  [[ -n "$CMUX_PANEL_ID" ]]     && flags="$flags --panel=$CMUX_PANEL_ID"
  echo "$flags"
}

# ── Scrollback restoration ────────────────────────────────────────────
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
  _cmux_send_bg "report_pwd \"$PWD\" $(_cmux_flags)"
}

# ── Fast git HEAD resolution ─────────────────────────────────────────
# Reads .git/HEAD directly without invoking `git` for speed.
_cmux_git_resolve_head_path() {
  local dir="$PWD"
  while [[ "$dir" != "/" && "$dir" != "" ]]; do
    if [[ -f "$dir/.git/HEAD" ]]; then
      echo "$dir/.git/HEAD"
      return 0
    elif [[ -f "$dir/.git" ]]; then
      local gitdir
      gitdir=$(<"$dir/.git")
      gitdir="${gitdir#gitdir: }"
      [[ "$gitdir" != /* ]] && gitdir="$dir/$gitdir"
      if [[ -f "$gitdir/HEAD" ]]; then
        echo "$gitdir/HEAD"
        return 0
      fi
    fi
    dir="${dir%/*}"
  done
  return 1
}

_cmux_git_read_branch_from_head() {
  local head_file="$1"
  [[ -f "$head_file" ]] || return 1
  local content
  content=$(<"$head_file" 2>/dev/null) || return 1
  if [[ "$content" == ref:\ refs/heads/* ]]; then
    echo "${content#ref: refs/heads/}"
    return 0
  fi
  echo "${content:0:8}"
  return 0
}

# ── Git branch (fast path + fallback, async with throttle) ───────────
_cmux_git_branch=""
_cmux_git_head_path=""
_cmux_git_last_report=0

# Core git branch detection — runs synchronously (used by async wrapper).
_cmux_update_git_branch_sync() {
  # Try fast path first
  if [[ -z "$_cmux_git_head_path" ]] || [[ ! -f "$_cmux_git_head_path" ]]; then
    _cmux_git_head_path=$(_cmux_git_resolve_head_path 2>/dev/null)
  fi

  local branch=""
  if [[ -n "$_cmux_git_head_path" ]]; then
    branch=$(_cmux_git_read_branch_from_head "$_cmux_git_head_path")
  fi

  if [[ -z "$branch" ]]; then
    branch=$(git symbolic-ref --short HEAD 2>/dev/null \
             || git rev-parse --short HEAD 2>/dev/null)
  fi

  if [[ -n "$branch" ]]; then
    local dirty=""
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
      dirty="*"
    fi
    _cmux_git_branch="$branch"
    _cmux_send "report_git_branch ${branch}${dirty} $(_cmux_flags)" \
      >/dev/null 2>&1
  elif [[ -n "$_cmux_git_branch" ]]; then
    _cmux_git_branch=""
    _cmux_git_head_path=""
    _cmux_send "clear_git_branch $(_cmux_flags)" >/dev/null 2>&1
  fi
}

# Async wrapper — runs in background, throttled to max once per 3 seconds.
_cmux_update_git_branch() {
  local now
  now=$(date +%s)
  if (( now - _cmux_git_last_report < 3 )); then
    return
  fi
  _cmux_git_last_report=$now

  _cmux_update_git_branch_sync &
  disown 2>/dev/null
}

# ── Async git HEAD watcher ────────────────────────────────────────────
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
    last_head=$(<"$head_file" 2>/dev/null)
    while true; do
      sleep 2
      local cur_head
      cur_head=$(<"$head_file" 2>/dev/null)
      if [[ "$cur_head" != "$last_head" ]]; then
        last_head="$cur_head"
        local branch="${cur_head#ref: refs/heads/}"
        if [[ "$branch" != "$cur_head" && -n "$branch" ]]; then
          _cmux_send "report_git_branch $branch $(_cmux_flags)" >/dev/null 2>&1
        fi
      fi
    done
  ) &
  _cmux_git_watcher_pid=$!
  disown 2>/dev/null
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

_cmux_pr_output_indicates_no_pr() {
  local output="$1" exit_code="$2"
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
          if [[ -n "$_cmux_pr_last_status" ]]; then
            _cmux_pr_last_status=""
            _cmux_send "clear_pr $(_cmux_flags)" >/dev/null 2>&1
          fi
        fi
      fi
    done
  ) &
  _cmux_pr_poll_pid=$!
  disown 2>/dev/null
}

# ── TTY reporting ────────────────────────────────────────────────────
_cmux_report_tty() {
  local tty_name
  tty_name=$(tty 2>/dev/null)
  [[ -n "$tty_name" ]] && _cmux_send_bg "report_tty $tty_name $(_cmux_flags)"
}

# ── Port scanning kick ──────────────────────────────────────────────
_cmux_ports_kick() {
  _cmux_send_bg "ports_kick"
}

# ── Shell state reporting ────────────────────────────────────────────
_cmux_report_prompt() {
  _cmux_send_bg "report_shell_state prompt $(_cmux_flags)"
}

_cmux_report_running() {
  _cmux_send_bg "report_shell_state running $(_cmux_flags)"
}

# ── Semantic prompt markers (OSC 133) ─────────────────────────────────
_cmux_osc133_prompt_start() {
  printf '\e]133;A;redraw=last;cl=line\a'
}

_cmux_osc133_command_start() {
  printf '\e]133;C\a'
}

_cmux_osc133_command_end() {
  printf '\e]133;D;%s\a' "$1"
}

# ── Process tree cleanup ─────────────────────────────────────────────
# Recursively collect all descendant PIDs of a given PID.
_cmux_child_pids() {
  local parent="$1"
  local children
  children=$(pgrep -P "$parent" 2>/dev/null) || return
  local pid
  for pid in $children; do
    echo "$pid"
    _cmux_child_pids "$pid"
  done
}

# Kill a process and all its descendants (leaf-first).
_cmux_kill_process_tree() {
  local pid="$1"
  local -a descendants
  mapfile -t descendants < <(_cmux_child_pids "$pid")
  # Kill children first (reverse order for leaf-first)
  local i
  for (( i=${#descendants[@]}-1; i>=0; i-- )); do
    kill "${descendants[$i]}" 2>/dev/null
  done
  kill "$pid" 2>/dev/null
}

_cmux_cleanup() {
  _cmux_stop_git_watcher
  if [[ -n "$_cmux_pr_poll_pid" ]]; then
    _cmux_kill_process_tree "$_cmux_pr_poll_pid"
    _cmux_pr_poll_pid=""
  fi
}
trap '_cmux_cleanup' EXIT

# ── Hook into bash prompt lifecycle ──────────────────────────────────
_cmux_last_exit=0
_cmux_preexec_fired=0

_cmux_prompt_command() {
  local exit_code=$?
  _cmux_last_exit=$exit_code
  _cmux_preexec_fired=0

  # Stop git watcher — command finished
  _cmux_stop_git_watcher

  # Invalidate cached HEAD path on directory change
  if [[ "$PWD" != "$_cmux_last_pwd" ]]; then
    _cmux_git_head_path=""
    _cmux_last_pwd="$PWD"
  fi

  # Semantic: mark end of previous command output
  _cmux_osc133_command_end "$exit_code"

  # Report state to cmux
  _cmux_report_pwd
  _cmux_update_git_branch
  _cmux_report_prompt

  # Semantic: mark start of prompt
  _cmux_osc133_prompt_start
}
_cmux_last_pwd="$PWD"

# Prepend to PROMPT_COMMAND (preserve existing commands)
if [[ -z "$PROMPT_COMMAND" ]]; then
  PROMPT_COMMAND="_cmux_prompt_command"
elif [[ "$PROMPT_COMMAND" != *"_cmux_prompt_command"* ]]; then
  PROMPT_COMMAND="_cmux_prompt_command;$PROMPT_COMMAND"
fi

# ── Preexec via PS0 (Bash 4.4+) ─────────────────────────────────────
# PS0 is evaluated and printed before each command runs. More reliable
# than DEBUG trap since it doesn't fire for subshells or PROMPT_COMMAND.
if [[ "${BASH_VERSINFO[0]}" -ge 5 ]] || \
   { [[ "${BASH_VERSINFO[0]}" -eq 4 ]] && [[ "${BASH_VERSINFO[1]}" -ge 4 ]]; }; then
  _cmux_ps0_preexec() {
    if [[ "$_cmux_preexec_fired" -eq 0 ]]; then
      _cmux_preexec_fired=1
      _cmux_osc133_command_start
      _cmux_report_running
      _cmux_start_git_watcher
    fi
  }
  PS0='$(_cmux_ps0_preexec)'
else
  # Fallback: DEBUG trap for older Bash
  _cmux_debug_trap() {
    case "$BASH_COMMAND" in
      _cmux_prompt_command|_cmux_report_pwd|_cmux_update_git_branch|\
      _cmux_report_prompt|_cmux_stop_git_watcher|_cmux_osc133_*|\
      _cmux_ps0_preexec|_cmux_cleanup)
        return ;;
    esac

    if [[ "$_cmux_preexec_fired" -eq 0 ]]; then
      _cmux_preexec_fired=1
      _cmux_osc133_command_start
      _cmux_report_running
      _cmux_start_git_watcher
    fi
  }
  trap '_cmux_debug_trap' DEBUG
fi

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
