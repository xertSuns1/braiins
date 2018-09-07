####################################################################################################
# generate files with Build ID information
####################################################################################################

# get timestamp
set time_sec [clock seconds]
set date_time [clock format $time_sec -format "%d.%m.%Y %H:%M:%S"]

puts "Build ID: ${time_sec} (${date_time})"

####################################################################################################
# generate VHDL file with unix timestamp
####################################################################################################
# name of the VHDL file
set filename "ip_repository/s9io_0.1/hdl/s9io_version.vhd"

# open the file for writing
set fd [open $filename "w"]

puts $fd [string repeat "-" 100]
puts $fd "-- Company:        Braiins Systems s.r.o."
puts $fd "-- Engineer:       Marian Pristach"
puts $fd "--"
puts $fd "-- Project Name:   S9 Board Interface IP"
puts $fd "-- Description:    Version of IP core as unix timestamp"
puts $fd "--"
puts $fd "-- Revision:       1.0.0 (${date_time})"
puts $fd "-- Comments:       This file is generated during synthesis process - do not modify manually!"
puts $fd [string repeat "-" 100]
puts $fd "library ieee;"
puts $fd "use ieee.std_logic_1164.all;"
puts $fd "use ieee.numeric_std.all;"
puts $fd ""
puts $fd "entity s9io_version is"
puts $fd "    port ("
puts $fd "        timestamp : out std_logic_vector(31 downto 0)"
puts $fd "    );"
puts $fd "end s9io_version;"
puts $fd ""
puts $fd "architecture rtl of s9io_version is"
puts $fd ""
puts $fd "begin"
puts $fd ""
puts $fd "	timestamp <= std_logic_vector(to_unsigned(${time_sec}, 32));"
puts $fd ""
puts $fd "end rtl;"

# close the file
close $fd

####################################################################################################
# generate build history file
####################################################################################################

# name of the build history file
set filename "build_history.txt"

# open the file for read/write
set fd [open $filename "a+"]

# read content ofthe file
seek $fd 0 start
set file_data [read $fd]

# put new record at beginning
seek $fd 0 start
puts $fd [string repeat "-" 80]
puts $fd "Build ${date_time} (${time_sec})"
puts $fd [string repeat "-" 80]
puts $fd [exec git log -1]
puts $fd ""

# put original file content
puts $fd $file_data

# close the file
close $fd

