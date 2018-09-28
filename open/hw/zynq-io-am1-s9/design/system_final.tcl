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
puts $fd [exec git log -1]
puts $fd ""

# check if git worktree is clean
set diff [exec git diff HEAD]
if { [string length $diff] > 0 } {
    puts $fd "Warning: git worktree is dirty! Check git diff log in build directory."
    puts $fd ""
}

# put original file content
puts -nonewline $fd $file_data

# close the file
close $fd

####################################################################################################
# Save git diff into file in build directory
####################################################################################################
if { [string length $diff] > 0 } {
    set filename "${projdir}/git.diff"
    set fd [open $filename "w"]
    puts $fd $diff
    close $fd
}

####################################################################################################
# Create backup of build directory
####################################################################################################
puts "Creating backup of build directory ..."
if ![file exists "backup"] {file mkdir "backup"}
file copy $projdir "backup/${projdir}_${build_id}"

####################################################################################################
# Final report
####################################################################################################
set elapsed_time [clock format [expr [clock seconds] - $build_id] -gmt 1 -format "%H:%M:%S"]
puts "Elapsed time: $elapsed_time"
