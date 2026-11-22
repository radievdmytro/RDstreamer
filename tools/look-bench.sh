#!/bin/bash
# Measures how fast, and at what processor cost, the Pi gets a 1080p60 picture
# through the GPU and onto HDMI in each way RDstreamer could do it.
# Run on the Pi with a screen on HDMI:  sudo bash look-bench.sh
# RDstreamer is stopped while it runs (it holds the screen) and started after.
set -u
FRAMES=600
CLIP=/tmp/rd-bench.h264

if ! command -v gst-launch-1.0 >/dev/null; then
  apt-get update -qq && apt-get install -y -qq gstreamer1.0-tools gstreamer1.0-gl >/dev/null
fi

systemctl stop ystreamer
trap 'systemctl start ystreamer' EXIT
sleep 1

if [ ! -s "$CLIP" ]; then
  echo "Making a 1080p60 test clip..."
  gst-launch-1.0 -q videotestsrc num-buffers=$FRAMES pattern=ball \
    ! video/x-raw,width=1920,height=1080,framerate=60/1,format=I420 \
    ! v4l2h264enc extra-controls="controls,video_bitrate=8000000" \
    ! 'video/x-h264,level=(string)4.2' ! h264parse ! filesink location=$CLIP
fi

DEC="filesrc location=$CLIP ! h264parse ! v4l2h264dec"
SINK="kmssink sync=false skip-vsync=true"
GL="glupload ! glshader ! gldownload ! video/x-raw(memory:DMABuf) ! $SINK"

# All processor time so far, and the busy part of it
stat_cpu() {
  awk 'NR==1 {t=0; for (i=2; i<=9; i++) t+=$i; print t, t-$5-$6}' /proc/stat
}

run() {
  local name=$1 pipe=$2 start end ticks0 ticks1 real cpu
  local busy0 busy1
  read -r ticks0 busy0 < <(stat_cpu)
  start=$(date +%s.%N)
  if timeout 60 gst-launch-1.0 -q $pipe >/tmp/rd-bench.log 2>&1; then
    end=$(date +%s.%N)
    read -r ticks1 busy1 < <(stat_cpu)
    real=$(echo "$end - $start" | bc -l)
    cpu=$(echo "100 * ($busy1 - $busy0) / ($ticks1 - $ticks0)" | bc -l)
    printf "%-34s %5.1f fps   processor %4.1f%% of all 4 cores\n" \
      "$name" "$(echo "$FRAMES / $real" | bc -l)" "$cpu"
  else
    printf "%-34s FAILED: %s\n" "$name" "$(grep -m1 -i error /tmp/rd-bench.log)"
  fi
}

command -v bc >/dev/null || apt-get install -y -qq bc >/dev/null
echo
run "no processing"                      "$DEC ! $SINK"
run "Quality 1080p (as now)"             "$DEC ! v4l2convert output-io-mode=dmabuf-import ! video/x-raw,format=RGBA ! queue max-size-buffers=2 ! $GL"
run "Quality 1080p, no copy in"          "$DEC ! v4l2convert output-io-mode=dmabuf-import capture-io-mode=dmabuf ! video/x-raw,format=RGBA ! queue max-size-buffers=2 ! $GL"
run "Speed 720p (as now)"                "$DEC ! v4l2convert output-io-mode=dmabuf-import ! video/x-raw,format=RGBA,height=720,pixel-aspect-ratio=1/1 ! queue max-size-buffers=2 ! $GL"
run "Speed 720p, no copy in"             "$DEC ! v4l2convert output-io-mode=dmabuf-import capture-io-mode=dmabuf ! video/x-raw,format=RGBA,height=720,pixel-aspect-ratio=1/1 ! queue max-size-buffers=2 ! $GL"
echo
echo "(The goggles send 60 fps: anything near 60 or above keeps up.)"
