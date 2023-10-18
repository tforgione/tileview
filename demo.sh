#!/usr/bin/env bash

rand() {
    shuf -i"$1"-"$2" -n1
}

iterations=$(rand 5 10)

for i in $(seq 1 "$iterations"); do
    color="\x1B[3$(rand 0 6)m"
    echo -e "$color$(rand 1 100)\x1b[0m"
    sleep $(rand 1 2)
done
