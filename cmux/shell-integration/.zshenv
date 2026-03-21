#!/usr/bin/env zsh
# cmux ZDOTDIR bootstrap — auto-injects shell integration, then restores
# the user's original ZDOTDIR so their own .zshenv/.zshrc run normally.
#
# How it works:
#   cmux sets ZDOTDIR to this directory. Zsh loads this .zshenv first.
#   We source the integration script, restore the real ZDOTDIR, then
#   source the user's actual .zshenv if it exists.

# Save our directory and restore the user's ZDOTDIR
_cmux_integration_dir="${ZDOTDIR}"

if [[ -n "$CMUX_ZSH_ZDOTDIR" ]]; then
  ZDOTDIR="$CMUX_ZSH_ZDOTDIR"
  unset CMUX_ZSH_ZDOTDIR
elif [[ -n "$CMUX_ZSH_ORIGINAL_ZDOTDIR" ]]; then
  # Sentinel value meaning "ZDOTDIR was unset"
  if [[ "$CMUX_ZSH_ORIGINAL_ZDOTDIR" == "__cmux_unset__" ]]; then
    unset ZDOTDIR
  else
    ZDOTDIR="$CMUX_ZSH_ORIGINAL_ZDOTDIR"
  fi
  unset CMUX_ZSH_ORIGINAL_ZDOTDIR
else
  unset ZDOTDIR
fi

# Source the cmux integration
if [[ -f "${_cmux_integration_dir}/cmux-zsh-integration.zsh" ]]; then
  source "${_cmux_integration_dir}/cmux-zsh-integration.zsh"
fi
unset _cmux_integration_dir

# Now source the user's real .zshenv if it exists
if [[ -n "$ZDOTDIR" ]]; then
  [[ -f "$ZDOTDIR/.zshenv" ]] && source "$ZDOTDIR/.zshenv"
else
  [[ -f "$HOME/.zshenv" ]] && source "$HOME/.zshenv"
fi
