#!/bin/sh

cat << EOF
  This is a combo preview of the new bitcoin mining software (bOSminer) and
  a demo of the new binary mining protocol (Stratum V2).

  How to run it:
   bosminer [FLAGS] [--pool <HOSTNAME:PORT> --user <USERNAME.WORKERNAME>]

  You can connect the miner to our Stratum V2 endpoint:
   bosminer --pool v2.stratum.slushpool.com:3336 --user <USERNAME.WORKERNAME>

  Follow the development on https://github.com/braiins/braiins
 -----------------------------------------------------------------------------

EOF
