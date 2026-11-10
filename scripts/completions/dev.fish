# Tab completion for `dev` (scripts/dev.py): task names, then each task's own
# subcommands, options and values.
#
# Install once (scripts/README.md, "Running a task"); fish loads
# completions/<command>.fish the first time the command is completed:
#   ln -s (git rev-parse --show-toplevel)/scripts/completions/dev.fish ~/.config/fish/completions/dev.fish
#
# How it works: this file only ever calls `dev` itself, with the request in the
# environment, so it does not care what `dev` is (a wrapper on PATH, a function) or
# where the checkout lives. dev.py answers with the task list, or hands over to the
# named task, where lib/complete.py answers from the script's real argparse parser or
# Typer app instead of running it. The candidates therefore cannot go stale.
# UV_OFFLINE keeps a Tab from ever downloading anything: before bootstrap has run
# there are simply no completions.

function __dev_complete
    set -l words (commandline -opc)
    set -l cur (commandline -ct)
    set -e words[1]
    # `dev --no-setup-check <task> …`: the flag is dev's, not the task's.
    if test (count $words) -gt 0; and test "$words[1]" = --no-setup-check
        set -e words[1]
    end

    if test (count $words) -eq 0
        HT_COMPLETE=1 HT_COMPLETE_CUR="$cur" UV_OFFLINE=1 dev 2>/dev/null
        return
    end

    set -l rest (string join \x1f -- $words[2..])
    for line in (HT_COMPLETE=1 HT_COMPLETE_WORDS="$rest" HT_COMPLETE_CUR="$cur" UV_OFFLINE=1 dev $words[1] 2>/dev/null)
        if test "$line" = __files__
            __fish_complete_path "$cur"
        else
            echo $line
        end
    end
end

complete -c dev -f -a '(__dev_complete)'
