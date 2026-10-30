# c2patool for the frames, isolated the way the differential collector isolates it
# (DIFFERENTIAL.md D-008): $C2PATOOL_SETTINGS and $C2PATOOL_TRUST_ANCHORS unset, an
# empty $XDG_CONFIG_HOME, and settings from scripts/c2patool-settings.py (built by
# differential.py's own code), so a c2pa.toml on this machine can never change what
# a frame prints.
#
#   source scripts/pub/c2p.fish          # after `cd $repo`
#   c2p FILE [c2patool args…]            # network off, no anchors (what the frames used)
#   C2P_TRUST=vendored c2p FILE          # anchors + TSA + EKU policy, as the ground truth
#   C2P_TRUST=eku c2p FILE               # EKU policy only
#   C2P_FETCH=1 c2p FILE                 # remote manifest fetch on; OCSP stays off
#   c2p_settings                         # path of the settings file c2p would use now
#
# Settings files are written once per mode and removed when the script exits.
# Never reads the clipboard.

set -g __c2p_scripts (realpath (status dirname)/..)
set -g __c2p_dir (mktemp -d -t c2p.XXXXXX)
mkdir -p $__c2p_dir/xdg

function __c2p_cleanup --on-event fish_exit
    rm -rf $__c2p_dir
end

function c2p_settings --description 'Path of the c2patool settings file for $C2P_TRUST / $C2P_FETCH'
    set -l trust none
    set -q C2P_TRUST; and test -n "$C2P_TRUST"; and set trust $C2P_TRUST
    if not contains -- $trust none eku vendored
        echo "c2p: C2P_TRUST must be none, eku or vendored (got '$trust')" >&2
        return 2
    end
    set -l args --trust $trust
    set -l tag $trust
    if set -q C2P_FETCH; and test "$C2P_FETCH" = 1
        set -a args --fetch
        set tag $tag-fetch
    end
    set -l s $__c2p_dir/$tag.toml
    if not test -s $s
        uv run --quiet $__c2p_scripts/c2patool-settings.py $args >$s
        or begin
            rm -f $s
            echo "c2p: could not build the $tag settings" >&2
            return 2
        end
    end
    echo $s
end

function c2p --description 'c2patool isolated from ambient config (D-008); C2P_TRUST=none|eku|vendored, C2P_FETCH=1'
    set -l s (c2p_settings); or return 2
    env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS XDG_CONFIG_HOME=$__c2p_dir/xdg \
        c2patool --settings $s $argv
end
