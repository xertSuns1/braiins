#!/bin/sh

# redirect STDOUT and STDERR to /dev/kmsg
exec 1<&- 2<&- 1>/dev/kmsg 2>&1

echo "System is running the bOSminer preview!"

green_led="/sys/class/leds/Green LED"
red_led="/sys/class/leds/Red LED"

# after successful boot, turn off the red LED and green LED let turned on
echo default-on > "$green_led/trigger"
echo none > "$red_led/trigger"
