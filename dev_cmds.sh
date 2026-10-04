#!/usr/bin/env bash
# Source this file to define project aliases in your current shell:
#   source env.sh

declare -A ALIASES
ALIASES=(
  [i]="Install or update to the latest"
  [b]="Build"
  [r]="Iterative build and run"
  [br]="Build Release"
  [lint]="Runs linter"
  [lintmore]="Runs linter"
  [cln]="Clean build artifacts"
  [ut]="Run unit tests"
  [func]="Run functional tests"
  [tstcv]="Run unit test coverage"
)

# Track which aliases were defined by load_alias
declare -A ALIASES_DEFINED

bold() {
  printf '\033[1m%s\033[0m' "$*"
}

load_alias() {
  # load_alias <name> <command-string>
  local name="$1"; shift
  local cmd="$1"; shift

  # define alias in the current (sourcing) shell
  alias "$name"="$cmd"
  ALIASES_DEFINED["$name"]=1

  local desc="${ALIASES[$name]:-(no description)}"
  printf "Loaded '%s'\t: %s\n" "$(bold "$name")" "$desc"
}

refresh_alias_status() {
  # Show aliases that are not defined
  for name in "${!ALIASES[@]}"; do
    if [ "${ALIASES_DEFINED[$name]:-0}" != "1" ]; then
      printf "N/A    '%s'\n" "$(bold "$name")"
    fi
  done
}

load_alias i "curl https://sh.rustup.rs -sSf | sh"
load_alias b "cargo build"
load_alias br "cargo build --release"
load_alias lint "cargo clippy"
load_alias lintmore "cargo clippy -- -W clippy::pedantic"
load_alias cln "cargo clean"
load_alias ut "cargo test"
load_alias func "cargo test --test functional"

refresh_alias_status