#!/usr/bin/env bash

for i in `seq 1 100`; do
    echo -en $i 'Compiling toto\r'
    sleep 0.02s
    echo -e $i 'Done\x1b[K'
    sleep 0.02s
    echo -en $i 'Compiling tata\r'
    sleep 0.02s
    echo -e $i 'Done\x1b[K'
    sleep 0.02s
done
