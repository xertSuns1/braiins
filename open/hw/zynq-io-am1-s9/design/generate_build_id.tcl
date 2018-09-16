####################################################################################################
# generate files with Build ID information
####################################################################################################

# get timestamp
set build_id [clock seconds]
set date_time [clock format $build_id -format "%d.%m.%Y %H:%M:%S"]

puts "Build ID: ${build_id} (${date_time})"

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
puts $fd "Build ${date_time} (${build_id})"
puts $fd [string repeat "-" 80]
puts $fd [exec git log -1]
puts $fd ""

# put original file content
puts -nonewline $fd $file_data

# close the file
close $fd

