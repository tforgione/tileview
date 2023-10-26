#!/usr/bin/env bash

echo -en "0"
for i in `seq 1 100`; do
    sleep 0.005s
    echo -en "\n$i"
done
