####################################################################################################
# Copyright (c) 2018 Braiins Systems s.r.o.
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.
####################################################################################################

####################################################################################################
timestamp "Finishing build ..."

####################################################################################################
# Generate build history file
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
puts $fd "Project:       $project"
puts $fd "Board:         $board"
puts $fd [string repeat "-" 80]
puts $fd [exec git log -1]
puts $fd ""

# check if git worktree is clean
set diff [exec git diff HEAD]
if { [string length $diff] > 0 } {
    puts $fd "Warning: git worktree is dirty! Check git diff log in build directory."
    puts $fd ""
}

puts $fd ""

# put original file content
puts -nonewline $fd $file_data

# close the file
close $fd

####################################################################################################
# Save git diff into file in build directory
####################################################################################################
if { [string length $diff] > 0 } {
    set filename [file join $projdir git.diff]
    set fd [open $filename "w"]
    puts $fd $diff
    close $fd
}

####################################################################################################
# Report utilization
####################################################################################################
# name of the report file
set filename [file join $projdir reports utilization_routed.rpt]

# open the file for read
set fd [open $filename "r"]

# extract line with Slice LUTs
while {[gets $fd line] >= 0} {
    if [regexp "Slice LUTs" $line] {
        set lineLUT $line
    }
    if [regexp "LUT as Memory" $line] {
        set lineLUTRAM $line
    }
    if [regexp "Slice Registers" $line] {
        set lineRegs $line
    }
    if [regexp "\\| Block RAM Tile" $line] {
        set lineBram $line
    }
    if [regexp "DSPs" $line] {
        set lineDSP $line
    }
}

# close the file
close $fd

# extract info about utilization from report
set numLUT [string trim [lindex [split $lineLUT "|"] 2]]
set percentLUT [string trim [lindex [split $lineLUT "|"] 5]]
set numLUTRAM [string trim [lindex [split $lineLUTRAM "|"] 2]]
set percentLUTRAM [string trim [lindex [split $lineLUTRAM "|"] 5]]
set numRegs [string trim [lindex [split $lineRegs "|"] 2]]
set percentRegs [string trim [lindex [split $lineRegs "|"] 5]]
set numBRAM [string trim [lindex [split $lineBram "|"] 2]]
set percentBRAM [string trim [lindex [split $lineBram "|"] 5]]
set numDSP [string trim [lindex [split $lineDSP "|"] 2]]
set percentDSP [string trim [lindex [split $lineDSP "|"] 5]]

# get timing parameters
set wns [format {%.3f} [get_property STATS.WNS [current_run]]]

# print report into console
puts "Device utilization:"
puts "LUT:     $numLUT\t ${percentLUT}%"
puts "LUT RAM: $numLUTRAM\t ${percentLUTRAM}%"
puts "FF:      $numRegs\t ${percentRegs}%"
puts "BRAM:    $numBRAM\t ${percentBRAM}%"
puts "DSP:     $numDSP\t ${percentDSP}%"
puts ""
puts "Timing:"
puts "WNS:     $wns ns"
puts [string repeat "-" 80]

# name of CSV file
set filename "report.csv"

# check if file exists
if { [file exists $filename] } {
    # open the file for append
    set fd [open $filename "a"]
} else {
    # open the file for write and create header
    set fd [open $filename "w"]
    puts -nonewline $fd {"Project Name","Top module","Build ID","Build Time","Device",}
    puts -nonewline $fd {"LUT [-]","LUT [%]",}
    puts -nonewline $fd {"LUT RAM [-]","LUT RAM [%]",}
    puts -nonewline $fd {"FF [-]","FF [%]",}
    puts -nonewline $fd {"BRAM [-]","BRAM [%]",}
    puts -nonewline $fd {"DSP [-]","DSP [%]",}
    puts $fd {"WNS [ns]"}
}

# print report to CSV file
puts -nonewline $fd "\"$project\",\"$top_module\",\"${build_id}\",\"${date_time}\",\"$partname\","
puts -nonewline $fd "$numLUT,$percentLUT,"
puts -nonewline $fd "$numLUTRAM,$percentLUTRAM,"
puts -nonewline $fd "$numRegs,$percentRegs,"
puts -nonewline $fd "$numBRAM,$percentBRAM,"
puts -nonewline $fd "$numDSP,$percentDSP,"
puts $fd "$wns"

# close the file
close $fd

####################################################################################################
# Final report
####################################################################################################
set elapsed_time [clock format [expr [clock seconds] - $build_id] -gmt 1 -format "%H:%M:%S"]
puts "Elapsed time: $elapsed_time"

####################################################################################################
# Create backup of build directory
####################################################################################################
puts "Creating backup of build directory ..."
set backup_dir [file join backup "${projdir}_${build_id}"]
set bitstream_src [file join $projdir results system.bit]
set bitstream_dst [file join backup "${design}_${board}.bit"]

if { ![file exists "backup"] } {
    file mkdir "backup"
}

if { [file exists $bitstream_dst] } {
    file delete -force $bitstream_dst
}

file copy $projdir $backup_dir
file copy $bitstream_src $bitstream_dst
