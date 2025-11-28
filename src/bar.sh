#!/bin/dash
interval=0

cpu() {
  cpu_val=$(grep -o "^[^ ]*" /proc/loadavg)

  printf "CPU"
  printf "$cpu_val"
}

language() {
  lang="$(xkb-switch)"
  printf "$lang"
}

wlan() {
	case "$(cat /sys/class/net/wl*/operstate 2>/dev/null)" in
	up) printf "󰤨" ;;
	down) printf "󰤭" ;;
	esac
}

mem() {
  printf " $(free -h | awk '/^Mem/ { print $3 }' | sed s/i//g)"
}

brightness() {
  printf " %.0f%%\n" $(light)
}

audio() {
  printf " %s\n" $(pactl get-sink-volume 0 | awk '{print $5}')
}

battery() {
  printf " $(cat /sys/class/power_supply/BAT0/capacity)%%"
}

clock() {
	printf "󱑆 $(date '+%Y, %b %d. %a, %H:%M:%S')"
}

while true; do
  if [ $interval = 0 ] || [ $(($interval % 60)) = 0 ]; then
    ~/.config/chadwm/scripts/battery.sh
  fi
  interval=$((interval + 1))

  sleep 1 && xsetroot -name "$(language) | $(wlan) | $(mem) | $(brightness) | $(audio) | $(battery) | $(clock)"
done
