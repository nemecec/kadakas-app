#!/bin/bash
# The measurement campaign of docs/modeler-limits.md. Usage:
#   CORE=<limits_measure test binary of kadakas-core> UI=<of kadakas-ui-core> PROPS=<of kadakas-props> \
#   OUT=<directory for the results> limits_campaign.sh STAGE
# STAGE is one of: a1 a2 a2big a3 b4 b5 b6 collab (first campaign) and big guards explode collab2
# (second campaign, with BASE: the binary of the code before the guards, and with the feature
# measure-limits on kadakas-core for the stages that raise the limits). Each line runs one
# scenario RUNS times in its own process (see limits_measure.py). Release binaries only.
set -u
PY="$(dirname "$0")/limits_measure.py"
mkdir -p "$OUT"
export TMPDIR_LM=${LM_TMP:-/tmp}
run() { # name binary runs scenario args
  local name=$1 bin=$2 runs=$3 scen=$4 args=$5
  { python3 "$PY" "$bin" "$runs" "$scen" "$args"; } >> "$OUT/$name.txt" 2>&1
}
{ echo "## stage $1 on $(date) uptime: $(uptime)"; } >> "$OUT/uptime.txt"
case "$1" in
a1)
  for a in "grid 221" "grid 222" "grid 223" "terrain 158" "terrain 159" "noisy 158" "noisy 159" \
           "sphere 112" "sphere 113" "spiky 112" "spiky 113" "boxes 6250" "boxes 6251"; do
    run a1_limit $CORE 5 limit "$a"
  done
  for a in "grid 40" "grid 80" "terrain 20" "terrain 40" "terrain 60" "sphere 20" "sphere 40" "noisy 40" "boxes 500" "boxes 2000"; do
    run a1_drawn $CORE 5 shape "$a"
  done
  for a in "terrain 80" "grid 120" "sphere 60" "boxes 6250"; do run a1_drawn $CORE 3 shape "$a"; done
  run a1_drawn $CORE 1 shape "terrain 158"
  ;;
a2)
  for k in 1000 2000 5000 10000 20000 50000; do run a2_empty $CORE 5 sweep "0 add_same $k"; done
  for k in 1000 5000 10000; do run a2_empty $CORE 5 sweep "0 add_tri $k"; done
  for k in 1000 10000; do run a2_empty $CORE 5 sweep "0 add_lines $k"; done
  for k in 1000 2000 5000 10000 20000 50000; do run a2_empty $CORE 5 sweep "0 erase $k"; done
  for k in 20000 50000; do run a2_empty $CORE 5 sweep_chunked "0 add_same $k"; done
  for k in 1000 2000 5000 10000 20000 50000; do run a2_80k $CORE 5 sweep "80k erase $k"; done
  run a2_80k $CORE 5 sweep "80k noop 0" 2>/dev/null
  for k in 1000 2000; do run a2_80k $CORE 5 sweep "80k add_same $k"; done
  run a2_80k $CORE 5 sweep "80k add_other 1000"
  run a2_80k $CORE 5 sweep "80k add_lines 1000"
  run a2_80k $CORE 5 sweep "80k add_tri 1000"
  run a2_80k $CORE 5 sweep "80k add_same 5000"
  run a2_80k $CORE 5 sweep "80k add_other 5000"
  run a2_80k $CORE 3 sweep "80k add_same 10000"
  run a2_80k $CORE 3 sweep "80k add_other 10000"
  run a2_80k $CORE 3 sweep "80k add_tri 10000"
  run a2_80k $CORE 1 sweep "80k add_same 20000"
  run a2_80k $CORE 1 sweep "80k add_same 50000"
  ;;
a3)
  for a in "grid 111" "grid 158" "grid 222" "terrain 91" "terrain 129" "terrain 158" "boxes 2000" "boxes 4000" "boxes 6250" "sphere 112"; do
    run a3_save $CORE 5 save "$a"
  done
  for a in boxes64 lines64x1 big10k; do run a3_log $CORE 5 logsize "$a"; done
  for a in "edit_surface grid 111" "edit_surface grid 158" "edit_surface grid 222" "edit_surface terrain 91" \
           "edit_surface terrain 129" "edit_surface terrain 158" "edit_surface boxes 6250" \
           "make_group grid 158" "make_group grid 222" "make_group terrain 158"; do
    run a3_collab $PROPS 5 collab "$a"
  done
  ;;
b4)
  for parts in 10 1000; do for n in 1000 5000 20000 100000; do run b4_instmem $CORE 5 instmem "cube $parts $n"; done; done
  ;;
b5)
  for p in 1000 5000 10000; do run b5_ops_sq $CORE 5 ops "sq $p 0"; run b5_ops_cube $CORE 5 ops "cube $p 0"; done
  run b5_explode $CORE 5 ops "sq 1000"
  run b5_explode $CORE 3 ops "cube 1000"
  run b5_explode $CORE 3 ops "sq 5000"
  run b5_explode $CORE 1 ops "sq 10000"
  run b5_explode $CORE 1 ops "cube 5000"
  run b5_explode $CORE 1 ops "cube 10000"
  run b5_walk $CORE 5 walk20k "sq"
  run b5_walk $CORE 5 walk20k "cube"
  run b5_walk $UI 5 wide "sq"
  run b5_walk $UI 5 wide "cube"
  for parts in 10 1000; do for n in 1000 5000 20000 100000; do run b5_infer $UI 5 infer "cube $parts $n"; done; done
  ;;
b6)
  run b6_nest $CORE 5 nest "2 30 1 sq"
  run b6_nest $CORE 5 nest "3 27 1 sq"
  run b6_nest $CORE 5 nest "3 28 1 sq"
  run b6_nest $CORE 5 nest "3 30 1 sq"
  run b6_nest $CORE 5 nest "15 2 1 sq"
  run b6_nest $CORE 5 nest "15 2 2 sq"
  run b6_nest $CORE 5 nest "15 1 1 sq"
  run b6_nest $CORE 5 nest "16 1 1 sq"
  run b6_over $CORE 5 oversize "3 30 1"
  run b6_over $CORE 5 oversize "15 2 2"
  run b6_over $CORE 5 oversize "2 141 1"
  for a in "6 8 empty" "7 8 empty" "8 8 empty" "9 8 empty" "10 8 empty" "15 2 empty" "12 3 empty" "8 8 sq" "9 8 sq"; do
    run b6_depth $CORE 5 depth_cost "$a"
  done
  for kib in 0 2048 1024 512 256 128 64 32; do run b6_stack $CORE 1 stack "$kib"; done
  for kib in 0 2048 1024 512 256 128 64 32; do run b6_stack_ui $UI 1 deep "$kib"; done
  ;;
collab)
  run b7_collab $PROPS 5 collab "move_instance cube 10 20000"
  run b7_collab $PROPS 5 collab "move_instance cube 1000 20000"
  for p in 1000 5000 10000; do run b7_collab $PROPS 5 collab "close_edit sq $p"; run b7_collab $PROPS 5 collab "close_edit cube $p"; done
  ;;
big)
  # Models above the current limits, measured with the limits multiplied by 3 (feature
  # measure-limits). The first three lines are at the current limits, for comparison.
  for a in "terrain 158" "grid 222" "sphere 112"; do LM_SCALE=1 run big_now $CORE 5 big "$a"; done
  for a in "terrain 194" "terrain 223" "grid 273" "grid 316" "sphere 194" "sphere 224" "spiky 194" "spiky 224"; do
    LM_SCALE=3 run big_raised $CORE 5 big "$a"
  done
  ;;
guards)
  for a in "6 8" "8 8" "10 8" "12 8"; do run g_chain_new $CORE 5 chain "$a"; done
  for a in "6 8 empty" "8 8 empty" "10 8 empty"; do run g_chain_old $BASE 3 depth_cost "$a"; done
  run g_chain_cmd $CORE 5 chain_cmd "10 8"
  run g_total $CORE 5 total_models "cube 10 20000"
  run g_total $CORE 5 total_models "cube 10 25000"
  run g_total_scene $UI 5 infer "cube 10 25000"
  run g_total_scene $UI 5 infer "cube 10 20000"
  run g_defsize $CORE 5 defsize "cubes"
  run g_defsize $CORE 5 defsize "text"
  ;;
explode)
  for a in "sq 1000" "sq 5000" "sq 10000" "cube 1000" "cube 3333" "cube 5000" "cube 10000"; do
    run g_explode_new $CORE 3 ops "$a"
  done
  for a in "sq 1000" "sq 5000" "cube 1000"; do run g_explode_old $BASE 3 ops "$a"; done
  for a in "sq 10000" "cube 3333" "cube 5000"; do run g_explode_old $BASE 1 ops "$a"; done
  ;;
collab2)
  for a in "edit_surface grid 222" "edit_surface terrain 158" "make_group grid 222" "make_group terrain 158"; do
    LM_SCALE=1 run c2_now $PROPS 5 collab "$a"
  done
  for a in "edit_surface terrain 223" "edit_surface grid 316" "make_group terrain 223" "make_group grid 316"; do
    LM_SCALE=3 run c2_raised $PROPS 5 collab "$a"
  done
  for a in "close_edit cube 8333" "close_edit sq 10000" "close_edit cube 10000"; do run c2_defs $PROPS 5 collab "$a"; done
  ;;
big2)
  for a in "sphere 137" "sphere 158" "spiky 137" "spiky 158"; do LM_SCALE=3 run big_raised $CORE 3 big "$a"; done
  ;;
collab3)
  for a in "edit_surface grid 215" "edit_surface terrain 150"; do LM_SCALE=1 run c2_now $PROPS 5 collab "$a"; done
  ;;
esac
